#![feature(iter_array_chunks)]
use futures::executor::ThreadPool;
use futures::future::{join_all, BoxFuture};
use futures::{executor, task::SpawnExt, FutureExt};
use tokio::join;
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

    async fn run_batch(&mut self, ops: Vec<WrappedOp<CounterOp>>) {
        let vl = self.value;
        fn reduce(l: i32, r: i32) -> i32 { l + r }
        let map = move |op: WrappedOp<CounterOp>| -> i32 {
            match op.0 {
                CounterOp::Get => {op.1(Some(vl)); 0},
                CounterOp::Incr => {op.1(None); 1},
            }
        };
        let det_res = 
          ops.iter().map(|op| match op.0 {
             CounterOp::Get => 0,
             CounterOp::Incr => 1,
         }).fold(0, |l,r| {l + r});
        let res = utils::parallel_reduce(ops, reduce, map).await;
        self.value += res;
    }
}


#[tokio::main]
async fn main() {
    let counter : Batcher<Counter> = Batcher::new();

    let mut tasks : Vec<_> =
        (1..1000).map(|i| {
            let counter_local = counter.clone();
            tokio::spawn(async move {
                if i % 10 == 0 {
                    let res = counter_local.run(CounterOp::Get).await;
                    println!("[task {}]: result of getting counter was {:?}", i, res)
                } else {
                    let res = counter_local.run(CounterOp::Incr).await;
                    println!("[task {}]: result of incr counter was {:?}", i, res)
                }
            })
        }).collect();

    let _ = join_all(tasks).await;
}
