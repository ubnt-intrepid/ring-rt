use crate::io::{Event, Handle};
use futures::channel::oneshot;
use std::{io, marker::PhantomPinned, os::unix::prelude::*, pin::Pin};

struct Read {
    fd: RawFd,
    tx: Option<oneshot::Sender<(Vec<u8>, io::Result<usize>)>>,
    buf: Option<Vec<u8>>,
    _pinned: PhantomPinned,
}

impl Event for Read {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        let buf = me.buf.as_deref_mut().unwrap();
        uring_sys::io_uring_prep_read(
            sqe.raw_mut(),
            me.fd,
            buf.as_mut_ptr().cast(),
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

pub async fn read(handle: &Handle, f: &impl AsRawFd, buf: Vec<u8>) -> (Vec<u8>, io::Result<usize>) {
    let (tx, rx) = oneshot::channel();
    let event = Read {
        fd: f.as_raw_fd(),
        tx: Some(tx),
        buf: Some(buf),
        _pinned: PhantomPinned,
    };
    handle.submit(event).await;
    rx.await.expect("canceled")
}
