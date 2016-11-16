extern crate fringe;
extern crate tokio_core;
#[macro_use(task_local)]
extern crate futures;

use futures::Async;

use std::cell::{RefCell, UnsafeCell};

/// Convenient macro to suspend and retry async operation
/// just as it was blocking operation
#[macro_export]
macro_rules! poll {
    ($e:expr) => {{
        let res;
        loop {
             match $e {
                Err(ref e) if e.kind() == ::std::io::ErrorKind::WouldBlock => {
                    $crate::yield_now();
                }
                other => {
                    res = Some(other);
                    break
                },
            }
        }
        res.unwrap()
        }}
}

task_local! {
    static TL_YIELDER : RefCell<Vec<YielderRefStore>> = RefCell::new(vec!())
}

// Unsafe magic to for task-local yidler
//
// TODO: Need wiser people to verify this not not buggy
//
// `fringe` gives coroutine `&'a mut Yielder<I, O>`
//
// to have access to it from `yield_now`, use
// an escape hatch and be extremely careful
struct YielderRefStore(UnsafeCell<&'static Yield>);

impl YielderRefStore {
    unsafe fn new<'a>(y : &'a Yield) -> YielderRefStore {
        YielderRefStore (
             UnsafeCell::new(std::mem::transmute(y))
        )
    }
}

unsafe impl Send for YielderRefStore {}

// Wrapper over `fringe::Yielder` to make put it in `task_local`
trait Yield {
    fn suspend(&self);
}

impl<I : Send, O : Send, E : Send> Yield for fringe::generator::Yielder<I, SuspendCommand<O, E>> {
    fn suspend(&self) {
        let _ = self.suspend(Ok(Async::NotReady));
    }
}
// We wake up coroutine passing this to it's inside
enum ResumeCommand {
    Unblock
}

// We suspend coroutine passing this to the outside
pub type SuspendCommand<O, E> = futures::Poll<O, E>;

unsafe fn yielder_tl_push(y : &Yield) {
    TL_YIELDER.with(|tl_yielder| {
        let mut tl_yielder = tl_yielder.borrow_mut();
        tl_yielder.push(YielderRefStore::new(y));
    });
}

unsafe fn yielder_tl_pop() -> &'static Yield {
    let yielder : YielderRefStore  = TL_YIELDER.with(|tl_yielder| {
        let mut tl_yielder = tl_yielder.borrow_mut();
        tl_yielder.pop().unwrap()
    });

    yielder.0.into_inner()
}


pub struct Fiber<'a, O : Send+'a, E : Send+'a> {
    co : fringe::Generator<'a, ResumeCommand, SuspendCommand<O, E>, fringe::OsStack>,
}

impl<'a, O : Send, E : Send> Fiber<'a, O, E> {
    pub fn new<F>(f : F) -> Self
        where
        F: FnOnce() -> Result<O, E> + Send +'a {
            let stack = fringe::OsStack::new(1 << 20).unwrap();
            let gen = fringe::Generator::new(stack, move |yielder, _resume_cmd : ResumeCommand| {
                unsafe { yielder_tl_push(yielder) };

                let res = f();

                // pop the `Yield`, but use the "typed" version
                let _pop_and_discard = unsafe { yielder_tl_pop() };
                // TODO: can any optimizations prevent `yielder` to see
                // changes introduced when task-local version was used?

                yielder.suspend(match res {
                    Ok(t) => Ok(Async::Ready(t)),
                    Err(e) => Err(e)
                });
            });

            Fiber {
                co : gen
            }
         }
}

impl<'a, O : Send, E : Send> futures::Future for Fiber<'a, O, E> {
    type Item = O;
    type Error = E;
    fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
        self.co.resume(ResumeCommand::Unblock).expect("poll on finished Fiber is illegal")
    }
}

/// Yield the current fiber
///
/// Block current fiber. It will be resumed and thus this function will return,
/// when the event loop decides it might be ready. This includes: previously
/// blocked IO becoming unblocked etc.
pub fn yield_now() {
    let y = unsafe { yielder_tl_pop() };

    y.suspend();

    unsafe { yielder_tl_push(y) }
}

/// Block current fiber to wait for result of another future
pub fn await<F: futures::Future>(mut f: F) -> Result<F::Item, F::Error> {
    loop {
        match f.poll() {
            Ok(Async::NotReady) => yield_now(),
            Ok(Async::Ready(val)) => return Ok(val),
            Err(e) => return Err(e),
        };
    }
}
