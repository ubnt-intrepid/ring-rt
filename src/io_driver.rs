use crate::{
    event::{Event, RawEvent},
    semaphore::{Permit, Semaphore},
};
use futures::channel::oneshot;
use iou::IoUring;
use std::{cell::RefCell, io, rc::Rc};

const SQ_CAPACITY: u32 = 16;

struct UserData {
    permit: Permit,
    tx: oneshot::Sender<*mut ()>,
    event: RawEvent,
}

struct Inner {
    ring: RefCell<IoUring>,
    sq_capacity: Semaphore,
}

impl Inner {
    async fn submit_event(&self, mut event: RawEvent) -> oneshot::Receiver<*mut ()> {
        let permit = self.sq_capacity.acquire().await;

        let mut ring = self.ring.borrow_mut();

        let mut sqe = ring.next_sqe().expect("SQ is full");
        unsafe {
            event.prepare(&mut sqe);
        }

        let (tx, rx) = oneshot::channel();
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
                sq_capacity: Semaphore::new(SQ_CAPACITY as usize),
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
        let rx = self.inner.submit_event(RawEvent::new(event)).await;
        let output = rx.await.unwrap();
        unsafe {
            let output = Box::from_raw(output as *mut E::Output);
            *output
        }
    }
}
