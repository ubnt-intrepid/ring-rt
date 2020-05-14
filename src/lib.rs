mod executor;

use futures::future::Future;

pub use crate::executor::{Event, Executor};

pub fn block_on<Fut: Future>(fut: Fut) -> Fut::Output {
    let mut executor = Executor::new().expect("failed to start executor");
    executor.block_on(fut)
}

pub async fn nop() -> std::io::Result<usize> {
    use futures::channel::oneshot;
    use std::io;
    use std::pin::Pin;

    struct NopEvent {
        tx: Option<oneshot::Sender<io::Result<usize>>>,
    }

    impl Event for NopEvent {
        unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
            uring_sys::io_uring_prep_nop(sqe.raw_mut());
        }

        unsafe fn complete(self: Pin<&mut Self>, cqe: &mut iou::CompletionQueueEvent<'_>) {
            let me = self.get_mut();
            let tx = me.tx.take().unwrap();
            let _ = tx.send(cqe.result());
        }
    }

    let acquire_permit = Executor::with_current(|exec| exec.acquire_permit());
    let permit = acquire_permit.await;

    let (tx, rx) = oneshot::channel();
    let nop_event = NopEvent { tx: Some(tx) };

    Executor::with_current(|exec| exec.submit_event(nop_event, permit));

    rx.await.expect("canceled")
}
