use super::event::Event;
use futures::channel::oneshot;
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

enum OpaqueEvent {}
enum OpaqueEventOutput {}

struct OpaqueEventVTable {
    prepare: unsafe fn(
        *mut OpaqueEvent, //
        &mut iou::SubmissionQueueEvent<'_>,
    ),
    complete: unsafe fn(
        *mut OpaqueEvent, //
        &mut iou::CompletionQueueEvent<'_>,
    ) -> *mut OpaqueEventOutput,
    drop: unsafe fn(*mut OpaqueEvent),
}

struct UserData {
    permit: Permit,
    tx: oneshot::Sender<*mut OpaqueEventOutput>,
    event: *mut OpaqueEvent,
    vtable: &'static OpaqueEventVTable,
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

    unsafe fn add_event(&self, user_data: Box<UserData>) {
        let mut ring = self.ring.borrow_mut();

        let mut sqe = ring.next_sqe().expect("SQ is full");
        (user_data.vtable.prepare)(user_data.event, &mut sqe);

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
                let user_data = *Box::from_raw(cqe.user_data() as *mut UserData);
                let output = (user_data.vtable.complete)(user_data.event, &mut cqe);
                let _ = user_data.tx.send(output);
                (user_data.vtable.drop)(user_data.event);
                drop(user_data.permit);
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

        let (tx, rx) = oneshot::channel();
        let user_data = Box::new(UserData {
            permit,
            tx,
            event: Box::into_raw(Box::new(event)) as *mut OpaqueEvent,
            vtable: &OpaqueEventVTable {
                prepare: prepare_fn::<E>,
                complete: complete_fn::<E>,
                drop: drop_fn::<E>,
            },
        });

        unsafe {
            self.inner.add_event(user_data);
        }

        let output = rx.await.unwrap();

        unsafe {
            let output = Box::from_raw(output as *mut E::Output);
            *output
        }
    }
}

unsafe fn prepare_fn<E: Event>(ptr: *mut OpaqueEvent, sqe: &mut iou::SubmissionQueueEvent<'_>) {
    let me: Pin<&mut E> = Pin::new_unchecked(&mut *(ptr as *mut E));
    <E as Event>::prepare(me, sqe);
}

unsafe fn complete_fn<E: Event>(
    ptr: *mut OpaqueEvent,
    cqe: &mut iou::CompletionQueueEvent<'_>,
) -> *mut OpaqueEventOutput {
    let me: Pin<&mut E> = Pin::new_unchecked(&mut *(ptr as *mut E));
    let result = <E as Event>::complete(me, cqe);
    Box::into_raw(Box::new(result)) as *mut OpaqueEventOutput
}

unsafe fn drop_fn<E: Event>(ptr: *mut OpaqueEvent) {
    std::ptr::drop_in_place(ptr as *mut E);
}
