use futures::task::SpawnExt;

pub async fn parallel_reduce<B, A : Default + Send + 'static, F, G>
    (pool: futures::executor::ThreadPool, batch: Vec<B>, f : F, g: G) -> A
    where F : (Fn(A, A) -> A) + Clone + Send + 'static,
          G : (Fn(B) -> A) {

    let batch : Vec<_> = batch.into_iter().map(g).collect();
    let mut results : Vec<_> =
            batch.into_iter().array_chunks::<10>().map(|c|{
                let f = f.clone();
                pool.clone().spawn_with_handle(async move {
                    c.into_iter().reduce(f).unwrap_or_default()
                }).unwrap()
            }).collect();
    while results.len() > 1 {
        results = results.into_iter().array_chunks::<10>().map(|c| {
            let f = f.clone();
            pool.clone().spawn_with_handle(async move {
                futures::future::join_all(c).await.into_iter().reduce(f).unwrap_or_default()
            }).unwrap()
        }).collect();
    }
    futures::future::join_all(results).await.into_iter().reduce(f).unwrap_or_default()
}
