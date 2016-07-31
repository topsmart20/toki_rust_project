#![allow(missing_docs)] // TODO: document this module

use std::io;
use std::sync::Arc;

use futures::stream::Stream;
use futures::{Future, Task, Poll};
use futures_io::Ready;

use IoFuture;
use event_loop::{IoSource, LoopHandle};
use readiness_stream::drop_source::DropSource;


// TODO: figure out a nicer way to factor this
mod drop_source {
    use event_loop::LoopHandle;

    pub struct DropSource {
        token: usize,
        loop_handle: LoopHandle,
    }

    impl DropSource {
        pub fn new(token: usize, loop_handle: LoopHandle) -> DropSource {
            DropSource {
                token: token,
                loop_handle: loop_handle,
            }
        }
    }

    // Safe because no public access exposed to LoopHandle; only used in drop
    unsafe impl Sync for DropSource {}

    impl Drop for DropSource {
        fn drop(&mut self) {
            self.loop_handle.drop_source(self.token)
        }
    }
}

pub struct ReadinessStream {
    io_token: usize,
    loop_handle: LoopHandle,
    source: IoSource,
    _drop_source: Arc<DropSource>,
}

impl ReadinessStream {
    pub fn new(loop_handle: LoopHandle, source: IoSource)
               -> Box<IoFuture<ReadinessStream>> {
        loop_handle.add_source(source.clone()).map(|token| {
            let drop_source = Arc::new(DropSource::new(token, loop_handle.clone()));
            ReadinessStream {
                io_token: token,
                source: source,
                loop_handle: loop_handle,
                _drop_source: drop_source,
            }
        }).boxed()
    }
}

impl Stream for ReadinessStream {
    type Item = Ready;
    type Error = io::Error;

    fn poll(&mut self, _task: &mut Task) -> Poll<Option<Ready>, io::Error> {
        match self.source.take_readiness() {
            None => Poll::NotReady,
            Some(r) => Poll::Ok(Some(r)),
        }
    }

    fn schedule(&mut self, task: &mut Task) {
        self.loop_handle.schedule(self.io_token, task)
    }
}

impl Drop for ReadinessStream {
    fn drop(&mut self) {
        self.loop_handle.deschedule(self.io_token)
    }
}
