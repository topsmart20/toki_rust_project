use super::File;

use futures::{Future, Poll};

use std::fs::File as StdFile;
use std::io;
use std::path::Path;

/// Future returned by `File::open` and resolves to a `File` instance.
#[derive(Debug)]
pub struct OpenFuture<P> {
    path: P,
}

impl<P> OpenFuture<P>
where P: AsRef<Path> + Send + 'static,
{
    pub(crate) fn new(path: P) -> Self {
        OpenFuture { path }
    }
}

impl<P> Future for OpenFuture<P>
where P: AsRef<Path> + Send + 'static,
{
    type Item = File;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let std = try_ready!(::blocking_io(|| {
            StdFile::open(&self.path)
        }));

        let file = File::from_std(std);
        Ok(file.into())
    }
}
