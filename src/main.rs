#![feature(iter_array_chunks)]
use futures::executor::ThreadPool;
use futures::future::BoxFuture;
use futures::{executor, task::SpawnExt, FutureExt};
use std::borrow::Borrow;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use futures::lock::Mutex;

mod promise;
mod utils;
mod batcher;

use promise::Promise;
use batcher::{Batcher, BatchedOp, Batched, WrappedOp};

#[derive(Debug)]
struct Counter { value : i32 }
#[derive(Debug)]
enum CounterOp { Get, Incr }


impl BatchedOp for CounterOp { type Res = Option<i32>; }

impl Batched for Counter {
    type Op = CounterOp;

    fn init() -> Self {
        Counter { value: 0}
    }

    async fn run_batch(&mut self, pool: futures::executor::ThreadPool, ops: Vec<WrappedOp<CounterOp>>) {
        println!("run batch of size {}", ops.len());
        let vl = self.value;
        fn reduce(l: i32, r: i32) -> i32 { l + r }
        let map = move |op: WrappedOp<CounterOp>| -> i32 {
            match op.0 {
                CounterOp::Get => {op.1(Some(vl)); 0},
                CounterOp::Incr => {op.1(None); 1},
            }
        };
        let res = utils::parallel_reduce(pool.clone(), ops, reduce, map).await;
        println!("result of update was {}", res);
        self.value += res;
    }
}



fn main() {
    let thread_pool = ThreadPool::new().unwrap();
    let counter : Batcher<Counter> = Batcher::new(thread_pool.clone());


    let mut tasks : Vec<_> =
        (1..100).map(|i| {
            let counter_local = counter.clone();
            thread_pool.spawn_with_handle(async move {
                if i % 10 == 0 {
                    let res = counter_local.run(CounterOp::Get).await;
                    println!("[task {}]: result of getting counter was {:?}", i, res)
                } else {
                    let res = counter_local.run(CounterOp::Incr).await;
                    println!("[task {}]: result of incr counter was {:?}", i, res)
                }
            }).unwrap()
        }).collect();


    thread::sleep(Duration::from_millis(5000))
    
    // futures::executor::block_on(futures::future::join_all(tasks));
}
