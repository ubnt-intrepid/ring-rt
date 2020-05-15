use crate::io::Operation;
use futures::channel::oneshot;
use std::{io, pin::Pin};

struct Nop {
    tx: Option<oneshot::Sender<io::Result<usize>>>,
}

impl Operation for Nop {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        uring_sys::io_uring_prep_nop(sqe.raw_mut());
    }

    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut iou::CompletionQueueEvent<'_>) {
        let me = self.get_mut();
        let tx = me.tx.take().unwrap();
        let _ = tx.send(cqe.result());
    }
}

pub async fn nop() -> io::Result<usize> {
    let acquire_permit = crate::executor::get(|exec| exec.acquire_permit());
    let permit = acquire_permit.await;

    let (tx, rx) = oneshot::channel();
    let nop_event = Nop { tx: Some(tx) };

    crate::executor::get(|exec| exec.submit_event(nop_event, permit));

    rx.await.expect("canceled")
}
