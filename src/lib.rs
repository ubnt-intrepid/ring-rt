mod executor;

use futures::future::Future;

pub use crate::executor::Executor;

pub fn block_on<Fut: Future>(fut: Fut) -> Fut::Output {
    let mut executor = Executor::new().expect("failed to start executor");
    executor.block_on(fut)
}

pub async fn nop() -> std::io::Result<usize> {
    let acquire_permit = Executor::with_current(|exec| exec.acquire_permit());
    let permit = acquire_permit.await;
    let handle = Executor::with_current(|exec| exec.nop(permit));
    handle.await
}
