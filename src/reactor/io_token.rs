use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io;

use futures::{Future, Poll};
use futures::task;
use mio;

use reactor::{Message, Handle, CoreFuture, Direction, Core};

/// A future which will resolve a unique `tok` token for an I/O object.
///
/// Created through the `Handle::add_source` method, this future can also
/// resolve to an error if there's an issue communicating with the event loop.
pub struct IoTokenNew<E> {
    inner: CoreFuture<(E, (Arc<AtomicUsize>, usize)), E>,
}

/// A token that identifies an active timeout.
pub struct IoToken {
    token: usize,
    // TODO: can we avoid this allocation? It's kind of a bummer...
    readiness: Arc<AtomicUsize>,
}

impl IoToken {
    /// Add a new source to an event loop, returning a future which will resolve
    /// to the token that can be used to identify this source.
    ///
    /// When a new I/O object is created it needs to be communicated to the
    /// event loop to ensure that it's registered and ready to receive
    /// notifications. The event loop with then respond back with the I/O object
    /// and a token which can be used to send more messages to the event loop.
    ///
    /// The token returned is then passed in turn to each of the methods below
    /// to interact with notifications on the I/O object itself.
    ///
    /// # Panics
    ///
    /// The returned future will panic if the event loop this handle is
    /// associated with has gone away, or if there is an error communicating
    /// with the event loop.
    pub fn new<E>(source: E, handle: &Handle) -> IoTokenNew<E>
        where E: mio::Evented + Send + 'static,
    {
        IoTokenNew {
            inner: CoreFuture {
                handle: handle.clone(),
                data: Some(source),
                result: None,
            },
        }
    }

	/// Consumes the last readiness notification the token this source is for
    /// registered.
	///
	/// Currently sources receive readiness notifications on an edge-basis. That
	/// is, once you receive a notification that an object can be read, you
	/// won't receive any more notifications until all of that data has been
	/// read.
	///
	/// The event loop will fill in this information and then inform futures
	/// that they're ready to go with the `schedule` method, and then the `poll`
	/// method can use this to figure out what happened.
    ///
    /// > **Note**: This method should generally not be used directly, but
    /// >           rather the `ReadinessStream` type should be used instead.
    // TODO: this should really return a proper newtype/enum, not a usize
    pub fn take_readiness(&self) -> usize {
        self.readiness.swap(0, Ordering::SeqCst)
    }

    /// Schedule the current future task to receive a notification when the
    /// corresponding I/O object is readable.
    ///
    /// Once an I/O object has been registered with the event loop through the
    /// `add_source` method, this method can be used with the assigned token to
    /// notify the current future task when the next read notification comes in.
    ///
    /// The current task will only receive a notification **once** and to
    /// receive further notifications it will need to call `schedule_read`
    /// again.
    ///
    /// > **Note**: This method should generally not be used directly, but
    /// >           rather the `ReadinessStream` type should be used instead.
    ///
    /// # Panics
    ///
    /// This function will panic if the event loop this handle is associated
    /// with has gone away, or if there is an error communicating with the event
    /// loop.
    ///
    /// This function will also panic if there is not a currently running future
    /// task.
    pub fn schedule_read(&self, handle: &Handle) {
        handle.send(Message::Schedule(self.token, task::park(), Direction::Read));
    }

    /// Schedule the current future task to receive a notification when the
    /// corresponding I/O object is writable.
    ///
    /// Once an I/O object has been registered with the event loop through the
    /// `add_source` method, this method can be used with the assigned token to
    /// notify the current future task when the next write notification comes
    /// in.
    ///
    /// The current task will only receive a notification **once** and to
    /// receive further notifications it will need to call `schedule_write`
    /// again.
    ///
    /// > **Note**: This method should generally not be used directly, but
    /// >           rather the `ReadinessStream` type should be used instead.
    ///
    /// # Panics
    ///
    /// This function will panic if the event loop this handle is associated
    /// with has gone away, or if there is an error communicating with the event
    /// loop.
    ///
    /// This function will also panic if there is not a currently running future
    /// task.
    pub fn schedule_write(&self, handle: &Handle) {
        handle.send(Message::Schedule(self.token, task::park(), Direction::Write));
    }

    /// Unregister all information associated with a token on an event loop,
    /// deallocating all internal resources assigned to the given token.
    ///
    /// This method should be called whenever a source of events is being
    /// destroyed. This will ensure that the event loop can reuse `tok` for
    /// another I/O object if necessary and also remove it from any poll
    /// notifications and callbacks.
    ///
    /// Note that wake callbacks may still be invoked after this method is
    /// called as it may take some time for the message to drop a source to
    /// reach the event loop. Despite this fact, this method will attempt to
    /// ensure that the callbacks are **not** invoked, so pending scheduled
    /// callbacks cannot be relied upon to get called.
    ///
    /// > **Note**: This method should generally not be used directly, but
    /// >           rather the `ReadinessStream` type should be used instead.
    ///
    /// # Panics
    ///
    /// This function will panic if the event loop this handle is associated
    /// with has gone away, or if there is an error communicating with the event
    /// loop.
    pub fn drop_source(&self, handle: &Handle) {
        handle.send(Message::DropSource(self.token));
    }
}

impl<E> Future for IoTokenNew<E>
    where E: mio::Evented + Send + 'static,
{
    type Item = (E, IoToken);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(E, IoToken), io::Error> {
        let res = try_ready!(self.inner.poll(|lp, io| {
            let pair = try!(lp.add_source(&io));
            Ok((io, pair))
        }, |io, slot| {
            Message::Run(Box::new(move |lp: &Core| {
                let res = lp.add_source(&io).map(|p| (io, p));
                slot.try_produce(res).ok()
                    .expect("add source try_produce intereference");
            }))
        }));

        let (io, (ready, token)) = res;
        Ok((io, IoToken { token: token, readiness: ready }).into())
    }
}
