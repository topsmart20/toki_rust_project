use std::io;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use net::udp::UdpSocket
use futures::{Async, Poll, Stream, Sink, StartSend, AsyncSink};
use futures::sync::BiLock;

use io::Io;

/// Encoding of frames via buffers.
///
/// This trait is used when constructing an instance of `FramedUdp`. It provides
/// one type: `Out` for encoding outgoing frames according to a protocol.
///
/// Because UDP is a connectionless protocol, the encode method will also be
/// responsible for determining the remote host to which the datagram should be
/// sent
///
/// The trait itself is implemented on a type that can track state for decoding
/// or encoding, which is particularly useful for streaming parsers. In many
/// cases, though, this type will simply be a unit struct (e.g. `struct
/// HttpCodec`).
pub trait EncodeUdp {

    /// The type of frames to be encoded.
    type Out;


    /// Encodes a frame into the buffer provided.
    ///
    /// This method will encode `msg` into the byte buffer provided by `buf`.
    /// The `buf` provided is an internal buffer of the `Framed` instance and
    /// will be written out when possible. 
    ///
    /// The codec also determines the destination to which the buffer should
    /// be directed, which will be returned as a SocketAddr;
    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> SocketAddr;
}

/// Decoding of frames via buffers.
///
/// This trait is used when constructing an instance of `FramedUdp`. It provides
/// one type: `In` for decoding incoming frames from a Datagram
///
/// Because UDP is a connectionless protocol, the decode method will also be
/// supplied with a SocketAddr of the remote host which sent the datagram
///
/// The trait itself is implemented on a type that can track state for decoding
/// or encoding, which is particularly useful for streaming parsers. In many
/// cases, though, this type will simply be a unit struct (e.g. `struct
/// HttpCodec`).
pub trait DecodeUdp {
    /// The type of decoded frames.
    type In;

    /// Attempts to decode a frame from the provided buffer of bytes.
    ///
    /// This method is called by `FramedUdp` on a single datagram which has been
    /// read from a socket. 
    ///
    /// It is required that the Decoder empty the read buffer in every call to
    /// decode, as the next poll_read that occurs will write the next datagram
    /// into the buffer, without regard for what is already there. 
    ///
    /// If the bytes look valid, but a frame isn't fully available yet, then
    /// `Ok(None)` is returned. This indicates to the `Framed` instance that
    /// it needs to read some more bytes before calling this method again.
    /// In such a case, it is the decoder's responsibility to copy the data
    /// into their own internal buffer for future use.
    ///
    /// Finally, if the bytes in the buffer are malformed then an error is
    /// returned indicating why. This informs `Framed` that the stream is now
    /// corrupt and should be terminated.
    ///
    /// When dealing with connectionless streams, there will likely be some sort
    /// of state machine. 
    fn decode(&mut self, src: &SocketAddr, buf: &mut Vec<u8>) -> Result<Option<Self::In>, io::Error>;
}

/// A unified `Stream` and `Sink` interface to an underlying `Io` object, using
/// the `Encode` and `Decode` traits to encode and decode frames.
///
/// You can acquire a `Framed` instance by using the `Io::framed` adapter.
pub struct FramedUdp<D, E> {
    socket: UdpSocket,
    encoder: E,
    decoder: D,
    out_addr : Option<SocketAddr>,
    rd: Vec<u8>,
    wr: Vec<u8>,
}

impl<D : DecodeUdp, E : EncodeUdp> Stream for Framed<D, E> {
    type Item = D::In;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<C::In>, io::Error> {
        loop {

            let before = self.rd.len();
            let ret = self.socket.recv_from(self.rd.mut_bytes(), &mut inaddr);
            match ret {
                Ok((n, addr)) => { 
                    trace!("read {} bytes", n);
                    trace!("attempting to decode a frame");
                    if let Some(frame) = try!(self.decoder.decode(&addr, &mut self.rd)) {
                        trace!("frame decoded from buffer");
                        self.rd.clear();
                        return Ok(Async::Ready(Some(frame)));
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if self.rd.len() == before {
                        return Ok(Async::NotReady)
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl<D : DecodeUdp, E : EncodeUdp> Sink for Framed<D, E> {
    type SinkItem = E::Out;
    type SinkError = io::Error;

    fn start_send(&mut self, item: C::Out) -> StartSend<E::Out, io::Error> {
        if self.wr.len() > 0 {
            try!(self.poll_complete());
            if self.wr.len() > 0 {
                return Ok(AsyncSink::NotReady(item));
            }
        }

        self.out_addr = Some(self.codec.encode(item, &mut self.wr));
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        trace!("flushing framed transport");

        while !self.wr.is_empty() {
            if let Some(outaddr) = self.out_addr.ref() {
                trace!("writing; remaining={}", self.wr.len());
                let n = try_nb!(self.socket.send_to(&self.wr, outaddr));
                self.wr.clear();
                self.out_addr = None;
                if n != self.wr.len() {
                    return Err(io::Error::new(io::ErrorKind::WriteZero,
                                              "failed to write frame datagram to socket"));
                }
            }
            else {
                return Err(io::Error::new(io::ErrorKind::Other,
                                          "outbound stream in invalid state: out_addr is not known"));
            }
        }

        return Ok(Async::Ready(()));
    }
}

pub fn framed_udp<D, E>(socket : UdpSocket, decoder : D, encoder : E) -> Framed<D, E> {
    Framed {
        socket: socket,
        encoder: encoder,
        decoder: decoder,
        rd: Vec::with_capacity(64 * 1024),
        wr: Vec::with_capacity(64 * 1024)
    }
}

impl<D, E> FramedUdp<D, E> {
    /// Splits this `Stream + Sink` object into separate `Stream` and `Sink`
    /// objects, which can be useful when you want to split ownership between
    /// tasks, or allow direct interaction between the two objects (e.g. via
    /// `Sink::send_all`).
    pub fn split(self) -> (FramedRead<D>, FramedWrite<E>) {
        let (a, b) = BiLock::new(self);
        let read = FramedUdpRead { framed: a };
        let write = FramedUdpWrite { framed: b };
        (read, write)
    }

    /// Returns a reference to the underlying I/O stream wrapped by `Framed`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn get_ref(&self) -> &UdpSocket {
        &self.socket
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by
    /// `Framed`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn get_mut(&mut self) -> &mut UdpSocket {
        &mut self.socket
    }

    /// Consumes the `Framed`, returning its underlying I/O stream.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn into_inner(self) -> UdpSocket {
        self.socket
    }
}
/// A `Stream` interface to an underlying `Io` object, using the `Decode` trait
/// to decode frames.
pub struct FramedRead<D, E> {
    framed: BiLock<Framed<D, E>>,
}

impl<D : DecodeUdp, E : EncodeUdp> Stream for FramedRead<D, E> {
    type Item = D::In;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<D::In>, io::Error> {
        if let Async::Ready(mut guard) = self.framed.poll_lock() {
            guard.poll()
        } else {
            Ok(Async::NotReady)
        }
    }
}

/// A `Sink` interface to an underlying `Io` object, using the `Encode` trait
/// to encode frames.
pub struct FramedWrite<D, E> {
    framed: BiLock<Framed<D, E>>,
}

impl<D : DecodeUdp, E : EncodeUdp> Sink for FramedWrite<D, E> {
    type SinkItem = E::Out;
    type SinkError = io::Error;

    fn start_send(&mut self, item: E::Out) -> StartSend<E::Out, io::Error> {
        if let Async::Ready(mut guard) = self.framed.poll_lock() {
            guard.start_send(item)
        } else {
            Ok(AsyncSink::NotReady(item))
        }
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        if let Async::Ready(mut guard) = self.framed.poll_lock() {
            guard.poll_complete()
        } else {
            Ok(Async::NotReady)
        }
    }
}

