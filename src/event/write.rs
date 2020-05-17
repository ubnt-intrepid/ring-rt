use crate::event::Event;
use std::{io, os::unix::prelude::*, pin::Pin};

struct Write {
    fd: RawFd,
    buf: Option<Vec<u8>>,
    offset: libc::off_t,
}

impl Event for Write {
    type Output = (Vec<u8>, io::Result<usize>);

    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        let buf = me.buf.as_deref().unwrap();
        uring_sys::io_uring_prep_write(
            sqe.raw_mut(),
            me.fd,
            buf.as_ptr().cast(),
            buf.len() as libc::c_uint,
            me.offset,
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

pub fn write(
    f: &impl AsRawFd,
    buf: Vec<u8>,
    offset: libc::off_t,
) -> impl Event<Output = (Vec<u8>, io::Result<usize>)> {
    Write {
        fd: f.as_raw_fd(),
        buf: Some(buf),
        offset,
    }
}
