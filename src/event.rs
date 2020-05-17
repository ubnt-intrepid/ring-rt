mod accept;
mod nop;
mod read;
mod write;

pub use accept::accept;
pub use nop::nop;
pub use read::read;
pub use write::write;

use iou::{CompletionQueueEvent, SubmissionQueueEvent};
use std::pin::Pin;

/// An I/O event handled by io_uring.
pub trait Event: Send + 'static {
    type Output: Send + 'static;

    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut SubmissionQueueEvent<'_>);
    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut CompletionQueueEvent<'_>) -> Self::Output;
}
