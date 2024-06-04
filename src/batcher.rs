use std::{fmt::Debug, sync::{atomic::{AtomicBool, Ordering}, Arc}};
use tokio::sync::mpsc;
use futures::{future::BoxFuture, lock::Mutex, FutureExt};

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

    fn run_batch(&mut self, ops: Vec<WrappedOp<Self::Op>>)
                 -> impl std::future::Future<Output = ()> + std::marker::Send;
}


struct BatcherInner<B : Batched> {
    data: Mutex<B>,
    recv: Mutex<mpsc::UnboundedReceiver<WrappedOp<B::Op>>>,
    send: Mutex<mpsc::UnboundedSender<WrappedOp<B::Op>>>,
    is_running: AtomicBool
}

pub struct Batcher<B: Batched>(Arc<BatcherInner<B>>);
unsafe impl<B: Batched> Send for Batcher<B> {}

impl<B: Batched> Clone for Batcher<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Batched + Send + 'static> Batcher<B> {
    pub fn new() -> Self {
        let (send, recv) = mpsc::unbounded_channel();
        Batcher(Arc::new(BatcherInner {
            data: Mutex::new(B::init()),
            recv: Mutex::new(recv),
            send: Mutex::new(send),
            is_running: AtomicBool::new(false),
        }))
    }

    fn try_launch(&self) -> BoxFuture<'static, ()> {
        let self_clone = self.clone();
        async move {
               if let Ok(_) = self_clone.0.is_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
                let mut recv = self_clone.0.recv.lock().await;
                let mut ops = vec![];
                if recv.recv_many(&mut ops, 100).await > 0 {
                    let mut data = self_clone.0.data.lock().await;
                    data.run_batch(ops).await;
                }
                drop(recv);
                self_clone.0.is_running.store(false, Ordering::SeqCst);
                tokio::spawn(async move {self_clone.try_launch().await}).await;
            } 
        }.boxed()
    }

    pub async fn run(&self, op : B::Op) -> <<B as Batched>::Op as BatchedOp>::Res {
        let (promise, set) = Promise::new();
        let wrapped_op = WrappedOp(op, Box::new(set));
        self.0.send.lock().await.send(wrapped_op).unwrap();

        let self_clone = self.clone();
        tokio::spawn(async move { self_clone.try_launch().await });
        promise.await
    }

}
