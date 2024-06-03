use std::{fmt::Debug, sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc}};
use futures::{executor::ThreadPool, future::BoxFuture, lock::Mutex, FutureExt};

use crate::promise::Promise;

pub trait BatchedOp {
    type Res : Send + 'static;
}

pub struct WrappedOp<Op : BatchedOp>(pub Op, pub Box<dyn FnOnce(Op::Res) -> () + Send>);

impl<Op: BatchedOp + Debug> Debug for WrappedOp<Op> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WrappedOp").field(&self.0).finish()
    }
}

pub trait Batched {
    type Op : BatchedOp + Send + Debug;

    fn init() -> Self;

    fn run_batch(&mut self, pool: futures::executor::ThreadPool, ops: Vec<WrappedOp<Self::Op>>)
                 -> impl std::future::Future<Output = ()> + std::marker::Send;
}


struct BatcherInner<B : Batched> {
    data: Mutex<B>,
    recv: Mutex<mpsc::Receiver<WrappedOp<B::Op>>>,
    send: Mutex<mpsc::Sender<WrappedOp<B::Op>>>,
    is_running: AtomicBool,
    pool: futures::executor::ThreadPool
}

pub struct Batcher<B: Batched>(Arc<BatcherInner<B>>);
unsafe impl<B: Batched> Send for Batcher<B> {}

impl<B: Batched> Clone for Batcher<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Batched + Send + 'static> Batcher<B> {
    pub fn new(pool: ThreadPool) -> Self {
        let (send, recv) = mpsc::channel();
        Batcher(Arc::new(BatcherInner {
            data: Mutex::new(B::init()),
            recv: Mutex::new(recv),
            send: Mutex::new(send),
            is_running: AtomicBool::new(false),
            pool,
        }))
    }

    fn try_launch(&self) -> BoxFuture<'static, ()> {
        let self_clone = self.clone();
        async move {
               if let Ok(_) = self_clone.0.is_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
                   let recv = self_clone.0.recv.lock().await;
                if let Ok(op) = recv.try_recv() {
                    let mut ops = vec![op];
                    while let Ok(op) = recv.try_recv() {
                        ops.push(op)
                    }
                    let pool_clone = self_clone.0.pool.clone();
                    let mut data = self_clone.0.data.lock().await;
                    data.run_batch(pool_clone, ops).await;
                }
                self_clone.0.is_running.store(false, Ordering::SeqCst);
                self_clone.try_launch().await                        
            } 
        }.boxed()
    }

    pub async fn run(&self, op : B::Op) -> <<B as Batched>::Op as BatchedOp>::Res {

        let (promise, set) = Promise::new();
        let wrapped_op = WrappedOp(op, Box::new(set));
        self.0.send.lock().await.send(wrapped_op).unwrap();

        let self_clone = self.clone();
        self.0.pool.spawn_ok(async move { self_clone.try_launch().await });
        promise.await
    }

}
