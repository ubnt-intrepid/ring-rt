use crate::io::{
    driver::{Driver, Handle as IoHandle},
    Event,
};
use crossbeam::channel::{Receiver, Sender};
use futures::{
    future::Future,
    task::{self, Poll},
};
use pin_utils::pin_mut;
use std::{io, pin::Pin};

pub struct JoinHandle<T>(async_task::JoinHandle<T, ()>);

impl<T> JoinHandle<T> {
    fn project(self: Pin<&mut Self>) -> Pin<&mut async_task::JoinHandle<T, ()>> {
        unsafe { self.map_unchecked_mut(|me| &mut me.0) }
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        let inner = self.project();
        match futures::ready!(inner.poll(cx)) {
            Some(val) => Poll::Ready(val),
            None => panic!("canceled or paniced"),
        }
    }
}

#[derive(Clone)]
pub struct Handle {
    io_handle: IoHandle,
    tx_pending_tasks: Sender<async_task::Task<()>>,
}

impl Handle {
    pub async fn submit<E: Event>(&self, event: E) -> E::Output {
        self.io_handle.submit(event).await
    }

    pub fn spawn<Fut>(&self, future: Fut) -> JoinHandle<Fut::Output>
    where
        Fut: Future + 'static,
    {
        let tx = self.tx_pending_tasks.clone();
        let schedule = move |task| tx.send(task).unwrap();
        let (task, handle) = async_task::spawn_local(future, schedule, ());
        task.schedule();
        JoinHandle(handle)
    }
}

pub struct Runtime {
    io_driver: Driver,
    rx_pending_tasks: Receiver<async_task::Task<()>>,
    handle: Handle,
}

impl Runtime {
    pub fn new() -> io::Result<Self> {
        let io_driver = Driver::new()?;
        let io_handle = io_driver.handle();
        let (tx_pending_tasks, rx_pending_tasks) = crossbeam::channel::unbounded();

        Ok(Self {
            io_driver,
            rx_pending_tasks,
            handle: Handle {
                io_handle,
                tx_pending_tasks,
            },
        })
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        loop {
            let mut cx = task::Context::from_waker(task::noop_waker_ref());
            let polled = fut.as_mut().poll(&mut cx);
            if let Poll::Ready(ret) = polled {
                return ret;
            }

            for task in self.rx_pending_tasks.try_iter() {
                task.run();
            }

            self.io_driver.submit_and_wait().expect("io_uring");
        }
    }
}
