# Batcher in Rust

This repository illustrates how the methodology from the batching
paper can be implemented in Rust --- the lack of GADTs means that
fewer constraints can be encoded into the type system and more have to
be checked at run time, but oh, well....

## Building & Running

Clone the project, and then run `cargo build` and then `cargo run`.

## Counter 

Let's implement a counter, the first example from the paper:

```rust
struct Counter { value : i32 }

enum CounterOp { Get, Incr }
```

In order to "batch" this DS, we'll need two trait impls:

First, a `BatchedOp` impl to relate operations to their results:
```rust
impl BatchedOp for CounterOp { type Res = Option<i32>; }
```

and secondly, a `Batched` impl, that actually implements a batched apply:

```rust
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
        let res = utils::parallel_reduce(ops, reduce, map).await;
        self.value += res;
    }
}
```

Here we use a little helper library `utils::parallel_reduce` to apply
the operations efficiently in parallel.

Having defined all this, we can then construct a batched version of this data structure:
```rust
let counter : Batcher<Counter> = Batcher::new();
```

and submit operations to it in direct style:
```rust
let res = counter_local.run(CounterOp::Get).await;
```

For example:
```rust
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
    println!("final counter value: {:?}", counter.run(CounterOp::Get).await);
}
```

## Batcher Implementation

The Batcher implementation mirrors the OCaml one:

```rust
pub async fn run(&self, op : B::Op) -> <<B as Batched>::Op as BatchedOp>::Res {
    // create a promise
    let (promise, set) = Promise::new();

    // wrap the operation with its continuation
    let wrapped_op = WrappedOp(op, Box::new(set));

    // send the operation to the
    self.0.send.lock().await.send(wrapped_op).unwrap();

    // call try_launch to start a worker unless one is already running
    let self_clone = self.clone();
    tokio::spawn(async move { self_clone.try_launch().await });
    promise.await
}
```
 

and `try_launch` mirrors the skeleton of our OCaml implementation, albeit, for simplicity omitting some of the timeout related operations:
```rust

fn try_launch(&self) -> BoxFuture<'static, ()> {

    let self_clone = self.clone();
    async move {
        // check if some worker is already running
        if let Ok(_) = self_clone.0.is_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            // if not running, promote self to worker
            let mut recv = self_clone.0.recv.lock().await;
            // receive all ops on queue
            let mut ops = vec![];
            if recv.recv_many(&mut ops, 100).await > 0 {
                let mut data = self_clone.0.data.lock().await;
                // run batch
                data.run_batch(ops).await;
            }
            // batch finished, register flag as no one running
            drop(recv);
            self_clone.0.is_running.store(false, Ordering::SeqCst);
            // queue another worker in case there are remaining tasks
            tokio::spawn(async move {self_clone.try_launch().await}).await;
        } 
    }.boxed()
}

```
