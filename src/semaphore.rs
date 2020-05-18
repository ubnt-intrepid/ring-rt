use futures::future::Future;
use futures_intrusive::sync::{Semaphore as RawSemaphore, SemaphoreReleaser};
use std::{mem::ManuallyDrop, sync::Arc};

pub struct Semaphore(Arc<RawSemaphore>);

impl Semaphore {
    pub fn new(permit: usize) -> Self {
        Self(Arc::new(RawSemaphore::new(true, permit)))
    }

    pub fn acquire(&self) -> impl Future<Output = Permit> + 'static {
        let sem = self.0.clone();
        async move {
            let releaser = unsafe {
                let sem_ref: &'static RawSemaphore = &*(&*sem as *const _);
                sem_ref.acquire(1).await
            };
            Permit {
                releaser: ManuallyDrop::new(releaser),
                sem: ManuallyDrop::new(sem),
            }
        }
    }
}

pub struct Permit {
    releaser: ManuallyDrop<SemaphoreReleaser<'static>>,
    sem: ManuallyDrop<Arc<RawSemaphore>>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.releaser);
            ManuallyDrop::drop(&mut self.sem);
        }
    }
}
