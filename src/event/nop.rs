use crate::event::Event;
use std::{io, pin::Pin};

struct Nop;

impl Event for Nop {
    type Output = io::Result<usize>;

    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        uring_sys::io_uring_prep_nop(sqe.raw_mut());
    }

    unsafe fn complete(
        self: Pin<&mut Self>,
        cqe: &mut iou::CompletionQueueEvent<'_>,
    ) -> Self::Output {
        cqe.result()
    }
}

pub fn nop() -> impl Event<Output = io::Result<usize>> {
    Nop
}
