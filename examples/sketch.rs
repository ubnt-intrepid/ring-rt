use futures::stream::{self, StreamExt as _};

fn main() {
    ring_rt::executor::block_on(main_async())
}

async fn main_async() {
    stream::iter(0..100usize)
        .for_each_concurrent(None, |_| async {
            let _ = ring_rt::io::nop().await;
        })
        .await;
}