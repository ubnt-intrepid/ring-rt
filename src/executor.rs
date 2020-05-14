use futures::{
    channel::oneshot,
    future::Future,
    task::{self, Poll},
};
use iou::IoUring;
use pin_utils::pin_mut;
use std::{cell::Cell, io, ptr::NonNull};

const NUM_MAX_SQES: u32 = 1u32 << 6;

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
    callback: Option<Box<dyn FnOnce(io::Result<usize>)>>,
}

pub struct Executor {
    ring: IoUring,
    capacity: usize, // FIXME: replace with futures_intrusive::sync::Semaphore
}

impl Executor {
    pub fn new() -> io::Result<Self> {
        let ring = IoUring::new(NUM_MAX_SQES)?;

        Ok(Self {
            ring,
            capacity: NUM_MAX_SQES as usize,
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

    pub(crate) fn nop(&mut self) -> impl Future<Output = io::Result<usize>> {
        let mut sqe = self.ring.next_sqe().expect("SQ is full");
        unsafe {
            uring_sys::io_uring_prep_nop(sqe.raw_mut());
        }

        let (tx, rx) = oneshot::channel();
        let user_data = Box::new(UserData {
            callback: Some(Box::new(|res| {
                let _ = tx.send(res);
            })),
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.capacity -= 1;

        async move { rx.await.expect("cancelled") }
    }

    pub fn block_on<Fut: Future>(&mut self, fut: Fut) -> Fut::Output {
        pin_mut!(fut);

        let waker = task::noop_waker();
        let mut cx = task::Context::from_waker(&waker);

        loop {
            let mut cnt = 0;
            while let Some(event) = self.ring.cq().peek_for_cqe() {
                let mut user_data = unsafe { Box::from_raw(event.user_data() as *mut UserData) };
                let callback = user_data.callback.take().expect("missing callback");
                callback(event.result());
                self.capacity += 1;
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
