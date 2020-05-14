use futures::stream::{self, StreamExt as _};

fn main() {
    ring_rt::block_on(async {
        stream::iter(0..100usize)
            .for_each_concurrent(None, |_| async {
                let _ = ring_rt::nop().await;
            })
            .await;
    })
}
