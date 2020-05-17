use crate::io::Event;
use std::{io, os::unix::prelude::*, pin::Pin};

struct Read {
    fd: RawFd,
    buf: Option<Vec<u8>>,
}

impl Event for Read {
    type Output = (Vec<u8>, io::Result<usize>);

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

    unsafe fn complete(
        self: Pin<&mut Self>,
        cqe: &mut iou::CompletionQueueEvent<'_>,
    ) -> Self::Output {
        let me = self.get_mut();
        let buf = me.buf.take().unwrap();
        (buf, cqe.result())
    }
}

pub fn read(f: &impl AsRawFd, buf: Vec<u8>) -> impl Event<Output = (Vec<u8>, io::Result<usize>)> {
    Read {
        fd: f.as_raw_fd(),
        buf: Some(buf),
    }
}
