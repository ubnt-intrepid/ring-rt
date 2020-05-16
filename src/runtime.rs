use crate::io::driver::{Driver, Handle};
use futures::{
    future::Future,
    task::{self, Poll},
};
use pin_utils::pin_mut;
use std::io;

pub struct Runtime {
    io_driver: Driver,
}

impl Runtime {
    pub fn new() -> io::Result<Self> {
        let io_driver = Driver::new()?;
        Ok(Self { io_driver })
    }

    pub fn io_handle(&self) -> Handle {
        self.io_driver.handle()
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        loop {
            let mut cx = task::Context::from_waker(task::noop_waker_ref());
            let polled = fut.as_mut().poll(&mut cx);
            if let Poll::Ready(ret) = polled {
                return ret;
            }

            self.io_driver.submit_and_wait().expect("io_uring");
        }
    }
}
