mod core;
use self::core::Cell;
pub(crate) use self::core::Header;

mod error;
#[allow(unreachable_pub)] // https://github.com/rust-lang/rust/issues/57411
pub use self::error::JoinError;

mod harness;
use self::harness::Harness;

cfg_rt_multi_thread! {
    mod inject;
    pub(super) use self::inject::Inject;
}

mod join;
#[allow(unreachable_pub)] // https://github.com/rust-lang/rust/issues/57411
pub use self::join::JoinHandle;

mod list;
pub(super) use self::list::OwnedTasks;

mod raw;
use self::raw::RawTask;

mod state;
use self::state::State;

mod waker;

use crate::future::Future;
use crate::util::linked_list;

use std::marker::PhantomData;
use std::ptr::NonNull;
use std::{fmt, mem};

/// An owned handle to the task, tracked by ref count
#[repr(transparent)]
pub(crate) struct Task<S: 'static> {
    raw: RawTask,
    _p: PhantomData<S>,
}

unsafe impl<S> Send for Task<S> {}
unsafe impl<S> Sync for Task<S> {}

/// A task was notified
#[repr(transparent)]
pub(crate) struct Notified<S: 'static>(Task<S>);

unsafe impl<S: Schedule> Send for Notified<S> {}
unsafe impl<S: Schedule> Sync for Notified<S> {}

/// Task result sent back
pub(crate) type Result<T> = std::result::Result<T, JoinError>;

pub(crate) trait Schedule: Sync + Sized + 'static {
    /// Bind a task to the executor.
    ///
    /// Guaranteed to be called from the thread that called `poll` on the task.
    /// The returned `Schedule` instance is associated with the task and is used
    /// as `&self` in the other methods on this trait.
    fn bind(task: Task<Self>) -> Self;

    /// The task has completed work and is ready to be released. The scheduler
    /// should release it immediately and return it. The task module will batch
    /// the ref-dec with setting other options.
    ///
    /// If the scheduler has already released the task, then None is returned.
    fn release(&self, task: &Task<Self>) -> Option<Task<Self>>;

    /// Schedule the task
    fn schedule(&self, task: Notified<Self>);

    /// Schedule the task to run in the near future, yielding the thread to
    /// other tasks.
    fn yield_now(&self, task: Notified<Self>) {
        self.schedule(task);
    }
}

cfg_rt! {
    /// Create a new task with an associated join handle
    pub(crate) fn joinable<T, S>(task: T) -> (Notified<S>, JoinHandle<T::Output>)
    where
        T: Future + Send + 'static,
        S: Schedule,
    {
        let raw = RawTask::new::<_, S>(task);

        let task = Task {
            raw,
            _p: PhantomData,
        };

        let join = JoinHandle::new(raw);

        (Notified(task), join)
    }
}

cfg_rt! {
    /// Create a new `!Send` task with an associated join handle
    pub(crate) unsafe fn joinable_local<T, S>(task: T) -> (Notified<S>, JoinHandle<T::Output>)
    where
        T: Future + 'static,
        S: Schedule,
    {
        let raw = RawTask::new::<_, S>(task);

        let task = Task {
            raw,
            _p: PhantomData,
        };

        let join = JoinHandle::new(raw);

        (Notified(task), join)
    }
}

impl<S: 'static> Task<S> {
    pub(crate) unsafe fn from_raw(ptr: NonNull<Header>) -> Task<S> {
        Task {
            raw: RawTask::from_raw(ptr),
            _p: PhantomData,
        }
    }

    pub(crate) fn header(&self) -> &Header {
        self.raw.header()
    }
}

cfg_rt_multi_thread! {
    impl<S: 'static> Notified<S> {
        pub(crate) unsafe fn from_raw(ptr: NonNull<Header>) -> Notified<S> {
            Notified(Task::from_raw(ptr))
        }
    }

    impl<S: 'static> Task<S> {
        pub(crate) fn into_raw(self) -> NonNull<Header> {
            let ret = self.header().into();
            mem::forget(self);
            ret
        }
    }

    impl<S: 'static> Notified<S> {
        pub(crate) fn into_raw(self) -> NonNull<Header> {
            self.0.into_raw()
        }
    }
}

impl<S: Schedule> Task<S> {
    /// Pre-emptively cancel the task as part of the shutdown process.
    pub(crate) fn shutdown(&self) {
        self.raw.shutdown();
    }
}

impl<S: Schedule> Notified<S> {
    /// Run the task
    pub(crate) fn run(self) {
        self.0.raw.poll();
        mem::forget(self);
    }

    /// Pre-emptively cancel the task as part of the shutdown process.
    pub(crate) fn shutdown(self) {
        self.0.shutdown();
    }
}

impl<S: 'static> Drop for Task<S> {
    fn drop(&mut self) {
        // Decrement the ref count
        if self.header().state.ref_dec() {
            // Deallocate if this is the final ref count
            self.raw.dealloc();
        }
    }
}

impl<S> fmt::Debug for Task<S> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "Task({:p})", self.header())
    }
}

impl<S> fmt::Debug for Notified<S> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "task::Notified({:p})", self.0.header())
    }
}

/// # Safety
///
/// Tasks are pinned
unsafe impl<S> linked_list::Link for Task<S> {
    type Handle = Task<S>;
    type Target = Header;

    fn as_raw(handle: &Task<S>) -> NonNull<Header> {
        handle.header().into()
    }

    unsafe fn from_raw(ptr: NonNull<Header>) -> Task<S> {
        Task::from_raw(ptr)
    }

    unsafe fn pointers(target: NonNull<Header>) -> NonNull<linked_list::Pointers<Header>> {
        // Not super great as it avoids some of looms checking...
        NonNull::from(target.as_ref().owned.with_mut(|ptr| &mut *ptr))
    }
}
