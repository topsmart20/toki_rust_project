extern crate futures;
extern crate futures_io;
extern crate futures_mio;

use std::net::TcpStream;
use std::thread;
use std::io::Write;

use futures::Future;
use futures::stream::Stream;
use futures_io::{chain, read_to_end};

macro_rules! t {
    ($e:expr) => (match $e {
        Ok(e) => e,
        Err(e) => panic!("{} failed with {:?}", stringify!($e), e),
    })
}

#[test]
fn chain_clients() {
    let mut l = t!(futures_mio::Loop::new());
    let srv = l.handle().tcp_listen(&"127.0.0.1:0".parse().unwrap());
    let srv = t!(l.run(srv));
    let addr = t!(srv.local_addr());

    let t = thread::spawn(move || {
        let mut s1 = TcpStream::connect(&addr).unwrap();
        s1.write_all(b"foo ").unwrap();
        let mut s2 = TcpStream::connect(&addr).unwrap();
        s2.write_all(b"bar ").unwrap();
        let mut s3 = TcpStream::connect(&addr).unwrap();
        s3.write_all(b"baz").unwrap();
    });

    let clients = srv.incoming().map(|e| e.0).take(3);
    let copied = clients.collect().and_then(|clients| {
        let mut clients = clients.into_iter();
        let a = clients.next().unwrap();
        let b = clients.next().unwrap();
        let c = clients.next().unwrap();

        let d = chain(a, b);
        let d = chain(d, c);
        read_to_end(d, Vec::new())
    });

    let data = t!(l.run(copied));
    t.join().unwrap();

    assert_eq!(data, b"foo bar baz");
}
