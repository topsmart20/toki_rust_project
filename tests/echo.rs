extern crate env_logger;
extern crate futures;
extern crate futures_io;
extern crate futures_mio;

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::thread;

use futures::Future;
use futures::stream::Stream;
use futures_io::copy;
use futures_mio::TcpStream;

macro_rules! t {
    ($e:expr) => (match $e {
        Ok(e) => e,
        Err(e) => panic!("{} failed with {:?}", stringify!($e), e),
    })
}

#[test]
fn echo_server() {
    drop(env_logger::init());

    let mut l = t!(futures_mio::Loop::new());
    let srv = l.handle().tcp_listen(&"127.0.0.1:0".parse().unwrap());
    let srv = t!(l.run(srv));
    let addr = t!(srv.local_addr());

    let msg = "foo bar baz";
    let t = thread::spawn(move || {
        use std::net::TcpStream;

        let mut s = TcpStream::connect(&addr).unwrap();

        for _i in 0..1024 {
            assert_eq!(t!(s.write(msg.as_bytes())), msg.len());
            let mut buf = [0; 1024];
            assert_eq!(t!(s.read(&mut buf)), msg.len());
            assert_eq!(&buf[..msg.len()], msg.as_bytes());
        }
    });

    let clients = srv.incoming();
    let client = clients.into_future().map(|e| e.0.unwrap()).map_err(|e| e.0);
    let halves = client.map(|s| {
        let s = Arc::new(s.0);
        (SocketIo(s.clone()), SocketIo(s))
    });
    let copied = halves.and_then(|(a, b)| copy(a, b));

    let amt = t!(l.run(copied));
    t.join().unwrap();

    assert_eq!(amt, msg.len() as u64 * 1024);
}

struct SocketIo(Arc<TcpStream>);

impl Read for SocketIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for SocketIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}
