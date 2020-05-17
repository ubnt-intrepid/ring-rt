use crate::event::{Event, RawEvent};
use futures::channel::oneshot;
use futures_intrusive::sync::{Semaphore, SemaphoreReleaser};
use iou::IoUring;
use std::{cell::RefCell, io, mem::ManuallyDrop, rc::Rc, sync::Arc};

const SQ_CAPACITY: u32 = 16;

struct Permit {
    releaser: ManuallyDrop<SemaphoreReleaser<'static>>,
    sq_capacity: ManuallyDrop<Arc<Semaphore>>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.releaser);
            ManuallyDrop::drop(&mut self.sq_capacity);
        }
    }
}

struct UserData {
    permit: Permit,
    tx: oneshot::Sender<*mut ()>,
    event: RawEvent,
}

struct Inner {
    ring: RefCell<IoUring>,
    sq_capacity: Arc<Semaphore>,
}

impl Inner {
    async fn acquire_permit(&self) -> Permit {
        let sq_capacity = self.sq_capacity.clone();
        let releaser = unsafe {
            let sq_capacity_ref: &'static Semaphore = &*(&*sq_capacity as *const Semaphore);
            sq_capacity_ref.acquire(1).await
        };
        Permit {
            releaser: ManuallyDrop::new(releaser),
            sq_capacity: ManuallyDrop::new(sq_capacity),
        }
    }

    fn add_event(&self, mut event: RawEvent, permit: Permit) -> oneshot::Receiver<*mut ()> {
        let mut ring = self.ring.borrow_mut();
        let mut sqe = ring.next_sqe().expect("SQ is full");

        let (tx, rx) = oneshot::channel();
        unsafe {
            event.prepare(&mut sqe);
        }

        let user_data = Box::new(UserData { permit, tx, event });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        rx
    }
}

pub struct Driver {
    inner: Rc<Inner>,
}

impl Driver {
    pub fn new() -> io::Result<Self> {
        let ring = IoUring::new(SQ_CAPACITY)?;

        Ok(Self {
            inner: Rc::new(Inner {
                ring: RefCell::new(ring),
                sq_capacity: Arc::new(Semaphore::new(true, SQ_CAPACITY as usize)),
            }),
        })
    }

    pub fn handle(&self) -> Handle {
        Handle {
            inner: self.inner.clone(),
        }
    }

    pub(crate) fn submit_and_wait(&mut self) -> io::Result<()> {
        let mut ring = self.inner.ring.borrow_mut();

        ring.sq().submit_and_wait(1)?;

        while let Some(mut cqe) = ring.cq().peek_for_cqe() {
            unsafe {
                let UserData {
                    mut event,
                    tx,
                    permit,
                } = *Box::from_raw(cqe.user_data() as *mut UserData);

                let output = event.complete(&mut cqe);
                let _ = tx.send(output);

                drop(permit);
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct Handle {
    inner: Rc<Inner>,
}

impl Handle {
    pub(crate) async fn submit<E: Event>(&self, event: E) -> E::Output
    where
        E: Event,
    {
        let permit = self.inner.acquire_permit().await;
        let rx = self.inner.add_event(RawEvent::new(event), permit);
        let output = rx.await.unwrap();
        unsafe {
            let output = Box::from_raw(output as *mut E::Output);
            *output
        }
    }
}
