mod nop;

pub use nop::nop;

use iou::{CompletionQueueEvent, SubmissionQueueEvent};
use std::pin::Pin;

pub trait Operation {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut SubmissionQueueEvent<'_>);
    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut CompletionQueueEvent<'_>);
}
