//! Hello world server.
//!
//! A simple client that opens a TCP stream, writes "hello world\n", and closes
//! the connection.
//!
//! You can test this out by running:
//!
//!     ncat -l 6142
//!
//! And then in another terminal run:
//!
//!     cargo run --example hello_world

#![deny(warnings, rust_2018_idioms)]
#![feature(async_await)]

use tokio;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[tokio::main]
pub async fn main() {
    let addr = "127.0.0.1:6142".parse().unwrap();

    // Open a TCP stream to the socket address.
    //
    // Note that this is the Tokio TcpStream, which is fully async.
    let mut stream = TcpStream::connect(&addr).await.unwrap();
    println!("created stream");
    let result = stream.write(b"hello world\n").await;
    println!("wrote to stream; success={:?}", result.is_ok());
}
