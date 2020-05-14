use futures::{
    future::Future,
    task::{self, Poll},
};
use pin_utils::pin_mut;

pub struct Executor {
    _p: (),
}

impl Executor {
    pub fn new() -> Self {
        Self { _p: () }
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        let waker = task::noop_waker();
        let mut cx = task::Context::from_waker(&waker);

        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(ret) => return ret,
                Poll::Pending => {
                    // TODO: wait until next event
                    continue;
                }
            }
        }
    }
}
