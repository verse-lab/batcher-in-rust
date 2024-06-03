use futures::executor;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

pub struct Promise<A> {
    result: Receiver<A>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl<A> Promise<A> {
    pub fn new() -> (Self, impl FnOnce(A) -> ()) {
        let (snd,rcv) = channel::<A>();
        let promise : Promise<A> = Promise { result: rcv, waker: Arc::new(Mutex::new(None)) };
        let waker = promise.waker.clone();
        let callback = move |res: A| -> () {
            snd.send(res);
            if let Some(waker) = waker.lock().unwrap().take() {
                waker.wake();
            }
        };
        (promise,callback)
    }
}

impl<A> Future for Promise<A> {
    type Output=A;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match self.result.try_recv() {
            Ok(res) => Poll::Ready(res),
            Err(_) => {
                self.waker.lock().unwrap().insert(cx.waker().clone());
                Poll::Pending
            },
        }
    }
}

