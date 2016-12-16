//! An echo server that just writes back everything that's written to it.
//!
//! If you're on unix you can test this out by in one terminal executing:
//!
//! ```sh
//! $ cargo run --example echo
//! ```
//!
//! and in another terminal you can run:
//!
//! ```sh
//! $ nc localhost 8080
//! ```
//!
//! Each line you type in to the `nc` terminal should be echo'd back to you!

extern crate env_logger;
extern crate futures;
extern crate tokio_core;
#[macro_use]
extern crate tokio_fiber;

use std::env;
use std::net::SocketAddr;

use futures::Future;
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;

use std::io::{self, Read, Write};

fn main() {
    env_logger::init().unwrap();
    let addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = addr.parse::<SocketAddr>().unwrap();

    // Create the event loop that will drive this server
    let mut l = Core::new().unwrap();
    let handle = l.handle();
    let remote = handle.remote().clone();

    // Create a TCP listener which will listen for incoming connections
    let mut socket = TcpListener::bind(&addr, &handle).unwrap();

    // Once we've got the TCP listener, inform that we have it
    println!("Listening on: {}", addr);

    let fiber = tokio_fiber::Fiber::new(move || -> io::Result<()> {
        loop {
            let (mut conn, _addr) = poll!(socket.accept())?;

            let conn_fib = tokio_fiber::Fiber::new(move || -> io::Result<()> {
                let mut buf = [0u8; 1024 * 64];
                loop {
                    let size = poll!(conn.read(&mut buf))?;
                    if size == 0 {/* eof */ break; }
                    let _ = poll!(conn.write_all(&mut buf[0..size]))?;
                }

                Ok(())
            });

            remote.spawn(|_handle| {
                conn_fib.map_err(|e| {
                    println!("error: {}", e);
                })
            });

        }
    });

    l.run(fiber.map_err(|e| {
        println!("error: {}", e);
    })).unwrap();
}
