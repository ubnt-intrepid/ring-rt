use futures::{
    prelude::*,
    select,
    stream::{self, FuturesUnordered},
};
use std::net::{TcpListener, TcpStream};

fn main() -> anyhow::Result<()> {
    ring_rt::executor::block_on(main_async())
}

async fn main_async() -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8000")?;

    let mut incoming = Box::pin(stream::repeat(()).then(|_| ring_rt::io::accept(&listener))).fuse();
    let mut tasks = FuturesUnordered::new();

    loop {
        select! {
            res = incoming.select_next_some() => {
                let (stream, addr) = res?;
                tasks.push(handle_connection(stream));
            },
            res = tasks.select_next_some() => res?,
            complete => break,
        }
    }

    Ok(())
}

async fn handle_connection(stream: TcpStream) -> anyhow::Result<()> {
    let _content = {
        let (mut buf, res) = ring_rt::io::read(&stream, vec![0u8; 8196]).await;
        let n = res?;
        assert!(n <= buf.len());
        unsafe {
            buf.set_len(n);
        }
        buf
    };

    let (_, res) = ring_rt::io::write(
        &stream,
        "\
            HTTP/1.1 200 OK\r\n\
            Server: ring-rt-example\r\n\
            \r\n\
        "
        .into(),
    )
    .await;
    res?;

    Ok(())
}
