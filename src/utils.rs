use futures::task::SpawnExt;
use itertools::Itertools;

pub async fn parallel_reduce<B, A : Default + Send + 'static, F, G>
    (batch: Vec<B>, f : F, g: G) -> A
    where F : (Fn(A, A) -> A) + Clone + Send + 'static,
          G : (Fn(B) -> A) {

    let batch : Vec<_> = batch.into_iter().map(g).collect();
    // let chunks = batch.into_iter().array_chunks::<10>();
    let mut results : Vec<_> =
            batch.into_iter().chunks(10).into_iter().map(|chunk| {
                let c : Vec<_> = chunk.into_iter().collect();
                let f = f.clone();
                tokio::spawn(async move {
                    c.into_iter().reduce(f).unwrap_or_default()
                })
            }).collect();

    while results.len() > 1 {
        results = results.into_iter().chunks(10).into_iter().map(|chunk| {
            let c : Vec<_> = chunk.into_iter().collect();
            let f = f.clone();
            tokio::spawn(async move {
                futures::future::join_all(c).await.into_iter().map(|v| v.unwrap()).reduce(f).unwrap_or_default()
            })
        }).collect();
    }
    futures::future::join_all(results).await.into_iter().map(|v| v.unwrap()).reduce(f).unwrap_or_default()
}
