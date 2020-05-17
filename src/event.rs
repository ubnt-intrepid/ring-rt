mod accept;
mod nop;
mod read;
mod write;

pub use accept::accept;
pub use nop::nop;
pub use read::read;
pub use write::write;

use iou::{CompletionQueueEvent, SubmissionQueueEvent};
use std::{pin::Pin, ptr};

/// An I/O event handled by io_uring.
pub trait Event: Send + 'static {
    type Output: Send + 'static;

    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut SubmissionQueueEvent<'_>);
    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut CompletionQueueEvent<'_>) -> Self::Output;
}

struct RawEventVTable {
    prepare: unsafe fn(*mut (), &mut SubmissionQueueEvent<'_>),
    complete: unsafe fn(*mut (), &mut CompletionQueueEvent<'_>) -> *mut (),
    drop: unsafe fn(*mut ()),
}

pub(crate) struct RawEvent {
    ptr: *mut (),
    vtable: &'static RawEventVTable,
}

impl RawEvent {
    pub(crate) fn new<E: Event>(event: E) -> Self {
        unsafe fn prepare<E: Event>(ptr: *mut (), sqe: &mut SubmissionQueueEvent<'_>) {
            let me: Pin<&mut E> = Pin::new_unchecked(&mut *(ptr as *mut E));
            <E as Event>::prepare(me, sqe);
        }

        unsafe fn complete<E: Event>(ptr: *mut (), cqe: &mut CompletionQueueEvent<'_>) -> *mut () {
            let me: Pin<&mut E> = Pin::new_unchecked(&mut *(ptr as *mut E));
            let result = <E as Event>::complete(me, cqe);
            Box::into_raw(Box::new(result)) as *mut ()
        }

        unsafe fn drop<E: Event>(ptr: *mut ()) {
            ptr::drop_in_place(ptr as *mut E);
        }

        Self {
            ptr: Box::into_raw(Box::new(event)) as *mut (),
            vtable: &RawEventVTable {
                prepare: prepare::<E>,
                complete: complete::<E>,
                drop: drop::<E>,
            },
        }
    }

    pub(crate) unsafe fn prepare(&mut self, sqe: &mut SubmissionQueueEvent<'_>) {
        (self.vtable.prepare)(self.ptr, sqe)
    }

    pub(crate) unsafe fn complete(&mut self, cqe: &mut CompletionQueueEvent<'_>) -> *mut () {
        (self.vtable.complete)(self.ptr, cqe)
    }
}

impl Drop for RawEvent {
    fn drop(&mut self) {
        unsafe {
            (self.vtable.drop)(self.ptr);
        }
    }
}
