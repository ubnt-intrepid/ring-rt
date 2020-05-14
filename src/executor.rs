use futures::{
    channel::oneshot,
    future::Future,
    task::{self, Poll},
};
use futures_intrusive::sync::{OwnedSemaphoreReleaser, Semaphore};
use iou::IoUring;
use pin_utils::pin_mut;
use std::{cell::Cell, io, ptr::NonNull, sync::Arc};

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

struct UserData {
    complete: Box<dyn FnOnce(io::Result<usize>)>,
    permit: OwnedSemaphoreReleaser,
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

    fn scope<R>(&mut self, f: impl FnOnce() -> R) -> R {
        let exec_ptr = TLS_EXEC.with(|exec| exec.replace(Some(NonNull::from(self))));
        let _guard = ResetOnDrop(exec_ptr);
        f()
    }

    pub(crate) fn with_current<R>(f: impl FnOnce(&mut Executor) -> R) -> R {
        let exec_ptr = TLS_EXEC.with(|exec| exec.take());
        let _guard = ResetOnDrop(exec_ptr);
        let mut exec_ptr = exec_ptr.expect("executor is not set on the current thread");
        f(unsafe { exec_ptr.as_mut() })
    }

    pub(crate) fn acquire_permit(&self) -> impl Future<Output = OwnedSemaphoreReleaser> + 'static {
        self.sem_sq_capacity.clone().acquire_owned(1)
    }

    pub(crate) fn nop(
        &mut self,
        permit: OwnedSemaphoreReleaser,
    ) -> impl Future<Output = io::Result<usize>> {
        //
        let mut sqe = self.ring.next_sqe().expect("SQ is full");

        unsafe {
            uring_sys::io_uring_prep_nop(sqe.raw_mut());
        }

        let (tx, rx) = oneshot::channel();
        let user_data = Box::new(UserData {
            callback: Box::new(|res| {
                let _ = tx.send(res);
            }),
            permit,
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        async move { rx.await.expect("cancelled") }
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        let waker = task::noop_waker();
        let mut cx = task::Context::from_waker(&waker);

        loop {
            let mut cnt = 0;
            while let Some(event) = self.ring.cq().peek_for_cqe() {
                let UserData { complete, permit } =
                    unsafe { *Box::from_raw(event.user_data() as *mut UserData) };
                drop(permit);
                complete(event.result());
                cnt += 1;
            }
            if cnt > 0 {
                eprintln!("receive {} CQEs", cnt);
            }

            match self.scope(|| fut.as_mut().poll(&mut cx)) {
                Poll::Ready(ret) => return ret,
                Poll::Pending => {
                    let cnt = self.ring.sq().submit().expect("failed to submit SQEs");
                    eprintln!("submit {} SQEs", cnt);
                    continue;
                }
            }
        }
    }
}
