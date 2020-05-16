use crate::io::{Event, Handle};
use futures::channel::oneshot;
use std::{io, pin::Pin};

struct Nop {
    tx: Option<oneshot::Sender<io::Result<usize>>>,
}

impl Event for Nop {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        uring_sys::io_uring_prep_nop(sqe.raw_mut());
    }

    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut iou::CompletionQueueEvent<'_>) {
        let me = self.get_mut();
        let tx = me.tx.take().unwrap();
        let _ = tx.send(cqe.result());
    }
}

pub async fn nop(handle: &Handle) -> io::Result<usize> {
    let (tx, rx) = oneshot::channel();
    handle.submit(Nop { tx: Some(tx) }).await;
    rx.await.expect("canceled")
}
