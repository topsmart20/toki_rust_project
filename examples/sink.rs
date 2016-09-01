//! A small server that writes as many nul bytes on all connections it receives.
//!
//! There is no concurrency in this server, only one connection is written to at
//! a time.

extern crate env_logger;
extern crate futures;
extern crate tokio_core;

use std::env;
use std::iter;
use std::net::SocketAddr;

use futures::Future;
use futures::stream::{self, Stream};
use tokio_core::io::IoFuture;

fn main() {
    env_logger::init().unwrap();
    let addr = env::args().nth(1).unwrap_or("127.0.0.1:8080".to_string());
    let addr = addr.parse::<SocketAddr>().unwrap();

    let mut l = tokio_core::Loop::new().unwrap();
    let server = l.handle().tcp_listen(&addr).and_then(|socket| {
        socket.incoming().and_then(|(socket, addr)| {
            println!("got a socket: {}", addr);
            write(socket).or_else(|_| Ok(()))
        }).for_each(|()| {
            println!("lost the socket");
            Ok(())
        })
    });
    println!("Listenering on: {}", addr);
    l.run(server).unwrap();
}

fn write(socket: tokio_core::TcpStream) -> IoFuture<()> {
    static BUF: &'static [u8] = &[0; 64 * 1024];
    let iter = iter::repeat(()).map(|()| Ok(()));
    stream::iter(iter).fold(socket, |socket, ()| {
        tokio_core::io::write_all(socket, BUF).map(|(socket, _)| socket)
    }).map(|_| ()).boxed()
}
