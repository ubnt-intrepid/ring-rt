use crate::io::Operation;
use futures::{
    future::Future,
    task::{self, Poll},
};
use futures_intrusive::sync::{OwnedSemaphoreReleaser, Semaphore};
use iou::IoUring;
use pin_utils::pin_mut;
use std::{cell::Cell, io, pin::Pin, ptr::NonNull, sync::Arc};

const SQ_CAPACITY: u32 = 16;

thread_local! {
    static TLS_EXEC: Cell<Option<NonNull<Executor>>> = Cell::new(None);
}

struct ResetOnDrop(Option<NonNull<Executor>>);

impl Drop for ResetOnDrop {
    fn drop(&mut self) {
        TLS_EXEC.with(|exec| exec.set(self.0.take()));
    }
}

fn set<R>(exec: &mut Executor, f: impl FnOnce() -> R) -> R {
    let exec_ptr = TLS_EXEC.with(|exec_tls| exec_tls.replace(Some(NonNull::from(exec))));
    let _guard = ResetOnDrop(exec_ptr);
    f()
}

pub(crate) fn get<R>(f: impl FnOnce(&mut Executor) -> R) -> R {
    let exec_ptr = TLS_EXEC.with(|exec| exec.take());
    let _guard = ResetOnDrop(exec_ptr);
    let mut exec_ptr = exec_ptr.expect("executor is not set on the current thread");
    f(unsafe { exec_ptr.as_mut() })
}

struct UserData {
    permit: OwnedSemaphoreReleaser,
    op: Pin<Box<dyn Operation + Send + 'static>>,
}

pub struct Executor {
    ring: IoUring,
    sem_sq_capacity: Arc<Semaphore>,
}

impl Executor {
    pub fn new() -> io::Result<Self> {
        let ring = IoUring::new(SQ_CAPACITY)?;

        Ok(Self {
            ring,
            sem_sq_capacity: Arc::new(Semaphore::new(true, SQ_CAPACITY as usize)),
        })
    }

    pub(crate) fn acquire_permit(&self) -> impl Future<Output = OwnedSemaphoreReleaser> + 'static {
        self.sem_sq_capacity.clone().acquire_owned(1)
    }

    pub(crate) fn submit_op(
        &mut self,
        permit: OwnedSemaphoreReleaser,
        op: Pin<Box<dyn Operation + Send + 'static>>,
    ) {
        let mut sqe = self.ring.next_sqe().expect("SQ is full");
        let mut user_data = Box::new(UserData { permit, op });
        unsafe {
            user_data.op.as_mut().prepare(&mut sqe);
        }
        sqe.set_user_data(Box::into_raw(user_data) as _);
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        loop {
            let mut cx = task::Context::from_waker(task::noop_waker_ref());
            let polled = set(self, || fut.as_mut().poll(&mut cx));
            if let Poll::Ready(ret) = polled {
                return ret;
            }

            self.ring
                .sq()
                .submit_and_wait(1)
                .expect("failed to submit SQEs");

            while let Some(mut cqe) = self.ring.cq().peek_for_cqe() {
                unsafe {
                    let UserData {
                        mut op, //
                        permit,
                    } = *Box::from_raw(cqe.user_data() as *mut UserData);
                    op.as_mut().complete(&mut cqe);
                    drop(permit);
                }
            }
        }
    }
}

pub fn block_on<Fut: Future>(fut: Fut) -> Fut::Output {
    let mut executor = Executor::new().expect("failed to start executor");
    executor.block_on(fut)
}
