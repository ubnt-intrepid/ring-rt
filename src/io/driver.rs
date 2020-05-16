use super::Event;
use futures_intrusive::sync::{Semaphore, SemaphoreReleaser};
use iou::IoUring;
use std::{cell::RefCell, io, mem::ManuallyDrop, pin::Pin, rc::Rc, sync::Arc};

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
    event: Pin<Box<dyn Event + Send + 'static>>,
    permit: Permit,
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

    async fn add_event(&self, event: Pin<Box<dyn Event + Send + 'static>>) {
        let permit = self.acquire_permit().await;

        let mut ring = self.ring.borrow_mut();
        let mut sqe = ring.next_sqe().expect("SQ is full");

        let mut user_data = Box::new(UserData { event, permit });
        unsafe {
            user_data.event.as_mut().prepare(&mut sqe);
        }
        sqe.set_user_data(Box::into_raw(user_data) as _);
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

        ring.sq().submit_and_wait(1).expect("failed to submit SQEs");

        while let Some(mut cqe) = ring.cq().peek_for_cqe() {
            unsafe {
                let UserData {
                    mut event, //
                    permit,
                } = *Box::from_raw(cqe.user_data() as *mut UserData);
                event.as_mut().complete(&mut cqe);
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
    pub(crate) async fn submit(&self, event: impl Event + Send + 'static) {
        self.inner.add_event(Box::pin(event)).await;
    }
}
