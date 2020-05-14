mod executor;

use crate::executor::Executor;
use futures::future::Future;

fn block_on<Fut: Future>(fut: Fut) -> Fut::Output {
    let mut executor = Executor::new();
    executor.block_on(fut)
}

fn main() {
    block_on(async {
        println!("Hello, world!");
    })
}
