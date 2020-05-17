use crate::{io::Event, runtime::Handle};
use futures::channel::oneshot;
use libc::{sockaddr_in, sockaddr_in6, sockaddr_storage, socklen_t};
use std::{
    io,
    marker::PhantomPinned,
    mem::{self, MaybeUninit},
    net::{self, SocketAddr, TcpListener, TcpStream},
    os::unix::prelude::*,
    pin::Pin,
};

// copied from libstd/sys_common/net.rs
fn sockaddr_to_addr(addr: &sockaddr_storage, len: usize) -> io::Result<SocketAddr> {
    match addr.ss_family as libc::c_int {
        libc::AF_INET => {
            assert!(len >= mem::size_of::<sockaddr_in>());
            let addr = unsafe { &*(addr as *const _ as *const sockaddr_in) };
            Ok(SocketAddr::from((
                addr.sin_addr.s_addr.to_ne_bytes(),
                addr.sin_port,
            )))
        }
        libc::AF_INET6 => {
            assert!(len >= mem::size_of::<sockaddr_in6>());
            let addr = unsafe { &*(addr as *const _ as *const sockaddr_in6) };
            Ok(SocketAddr::V6(net::SocketAddrV6::new(
                net::Ipv6Addr::from(addr.sin6_addr.s6_addr),
                addr.sin6_port,
                addr.sin6_flowinfo,
                addr.sin6_scope_id,
            )))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid argument",
        )),
    }
}

struct Accept {
    fd: RawFd,
    tx: Option<oneshot::Sender<io::Result<(TcpStream, SocketAddr)>>>,
    addr: MaybeUninit<sockaddr_storage>,
    addrlen: socklen_t,
    _pinned: PhantomPinned,
}

impl Event for Accept {
    unsafe fn prepare(self: Pin<&mut Self>, sqe: &mut iou::SubmissionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        uring_sys::io_uring_prep_accept(
            sqe.raw_mut(),
            me.fd,
            me.addr.as_mut_ptr().cast(),
            &mut me.addrlen,
            0,
        );
    }

    unsafe fn complete(self: Pin<&mut Self>, cqe: &mut iou::CompletionQueueEvent<'_>) {
        let me = self.get_unchecked_mut();
        let tx = me.tx.take().unwrap();
        let _ = tx.send(cqe.result().and_then(|n| {
            let stream = TcpStream::from_raw_fd(n as _);
            let addr = sockaddr_to_addr(&*me.addr.as_ptr(), me.addrlen as usize)?;
            Ok((stream, addr))
        }));
    }
}

pub async fn accept(
    handle: &Handle,
    listener: &TcpListener,
) -> io::Result<(TcpStream, SocketAddr)> {
    let (tx, rx) = oneshot::channel();
    let event = Accept {
        fd: listener.as_raw_fd(),
        tx: Some(tx),
        addr: MaybeUninit::uninit(),
        addrlen: mem::size_of::<sockaddr_storage>() as socklen_t,
        _pinned: PhantomPinned,
    };
    handle.io_handle().submit(event).await;
    rx.await.expect("canceled")
}
