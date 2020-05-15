use futures::{
    prelude::*,
    select,
    stream::{self, FuturesUnordered},
};
use std::net::TcpListener;

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
                println!("connect a TCP connection to {}", addr);
                tasks.push(fallible(async move {
                    let _ = stream;
                    Ok(())
                }));
            },
            res = tasks.select_next_some() => {
                println!("shutdown");
                res?;
            }
            complete => break,
        }
    }

    Ok(())
}

fn fallible<Fut: Future<Output = anyhow::Result<()>>>(fut: Fut) -> Fut {
    fut
}
