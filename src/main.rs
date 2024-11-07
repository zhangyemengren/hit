use std::{sync::Arc, time::{Duration, Instant}};
use tokio::sync::Semaphore;
use reqwest::ClientBuilder;
// 并行请求的最大数量
// qps
// get 请求
// 秒数
// tui
// 获取参数

const MAX_CONCURRENT_REQUESTS: usize = 4;
const REQUEST_TIMEOUT: u64 = 1;
const REQUEST_DURATION_SECS: u64 = 2;

#[tokio::main]
async fn main() {
    let now = Instant::now();
    let client = ClientBuilder::new().timeout(Duration::from_secs(REQUEST_TIMEOUT)).build().unwrap();
    let client = Arc::new(client);
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    println!("over");
    loop {
        if now.elapsed().as_secs() >= REQUEST_DURATION_SECS {
            break;
        }
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        tokio::spawn(async move{
            let _permit = semaphore.acquire().await.unwrap();
            let res = client.get("http://localhost:8000")
                .send()
                .await
                .unwrap();
            println!("Status: {}", res.text().await.unwrap());
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            drop(_permit);
        });
    }
    println!("Done!");
}