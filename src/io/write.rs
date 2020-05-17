use crate::{io::Event, runtime::Handle};
use futures::channel::oneshot;
use std::{io, marker::PhantomPinned, os::unix::prelude::*, pin::Pin};

struct Write {
    fd: RawFd,
    tx: Option<oneshot::Sender<(Vec<u8>, io::Result<usize>)>>,
    buf: Option<Vec<u8>>,
    _pinned: PhantomPinned,
}

impl Event for Write {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        let buf = me.buf.as_deref().unwrap();
        uring_sys::io_uring_prep_write(
            sqe.raw_mut(),
            me.fd,
            buf.as_ptr().cast(),
            buf.len() as libc::c_uint,
            0,
        );
    }

    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut iou::CompletionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        let tx = me.tx.take().unwrap();
        let buf = me.buf.take().unwrap();
        let _ = tx.send((buf, cqe.result()));
    }
}

pub async fn write(
    handle: &Handle,
    f: &impl AsRawFd,
    buf: Vec<u8>,
) -> (Vec<u8>, io::Result<usize>) {
    let (tx, rx) = oneshot::channel();
    let event = Write {
        fd: f.as_raw_fd(),
        tx: Some(tx),
        buf: Some(buf),
        _pinned: PhantomPinned,
    };
    handle.io_handle().submit(event).await;
    rx.await.expect("canceled")
}
