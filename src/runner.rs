use clap::Parser;
use reqwest::{Client, ClientBuilder};
use std::{
    process,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    signal,
    sync::{mpsc, Semaphore},
};
use tokio_util::sync::CancellationToken;

const DEFAULT_MAX_CONCURRENT: usize = 50;
const DEFAULT_REQUEST_TIMEOUT: u64 = 1;
const DEFAULT_REQUESTS: u64 = 200;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Request duration(seconds)
    #[arg(short, long)]
    duration: Option<u64>,
    /// Number of requests, ignored if duration exists
    #[arg(short, default_value_t = DEFAULT_REQUESTS)]
    n_requests: u64,
    /// QPS
    #[arg(short)]
    qps: Option<u64>,
    /// Max concurrent requests
    #[arg(short, default_value_t = DEFAULT_MAX_CONCURRENT)]
    max_concurrent: usize,
    /// Target url
    url: String,
}

pub struct Runner {
    now: Instant,
    client: Arc<Client>,
    semaphore: Arc<Semaphore>,
    request_count: Arc<AtomicU64>,
    average_duration: Arc<AtomicU64>,
    args: Args,
}

impl Runner {
    pub fn new(args: Args) -> Self {
        Runner {
            now: Instant::now(),
            client: Arc::new(
                ClientBuilder::new()
                    .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT))
                    .build()
                    .unwrap(),
            ),
            semaphore: Arc::new(Semaphore::new(args.max_concurrent)),
            request_count: Arc::new(AtomicU64::new(0)),
            average_duration: Arc::new(AtomicU64::new(0)),
            args,
        }
    }

    pub async fn run(&self) {
        let cancellation_token = CancellationToken::new();
        let mut handles = vec![];
        loop {
            if self.is_done() {
                cancellation_token.cancel();
                break;
            }
            let client = Arc::clone(&self.client);
            let semaphore = Arc::clone(&self.semaphore);
            let request_count = Arc::clone(&self.request_count);
            let url = self.args.url.clone();
            let token = cancellation_token.clone();
            let diff = self.diff_qps_duration();
            if diff > 0 {
                tokio::time::sleep(Duration::from_millis(diff)).await;
            }
            let (tx, mut rx) = mpsc::unbounded_channel();
            let handle = tokio::spawn(async move {
                tokio::select! {
                    _ = signal::ctrl_c() => {
                        process::exit(0);
                    }
                    _ = token.cancelled() => {}
                    _ = async {
                        let _permit = semaphore.acquire().await.unwrap();
                        request_count.fetch_add(1, Ordering::SeqCst);
                        let start = Instant::now();
                        let _res = client.get(url)
                            .send()
                            .await
                            .unwrap();
                        let duration = start.elapsed().as_millis() as u64;
                        tx.send(duration).unwrap();
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        drop(_permit);
                    } => {}
                }
            });
            let duration = rx.recv().await.unwrap();
            self.set_average_duration(duration);
            handles.push(handle);
        }
        for handle in handles {
            handle.await.unwrap();
        }
        println!("Done!");
    }
    fn is_done(&self) -> bool {
        if let Some(duration) = self.args.duration {
            self.now.elapsed().as_secs() >= duration
        } else {
            self.request_count.load(Ordering::SeqCst) >= self.args.n_requests
        }
    }
    fn set_average_duration(&self, duration: u64) {
        let request_count = self.request_count.load(Ordering::SeqCst);
        let average_duration = self.average_duration.load(Ordering::SeqCst);
        let new_average_duration =
            (average_duration * (request_count - 1) + duration) / request_count;
        self.average_duration
            .store(new_average_duration, Ordering::SeqCst);
    }
    fn diff_qps_duration(&self) -> u64 {
        let average_duration = self.average_duration.load(Ordering::SeqCst) as i128;
        let Some(qps) = self.args.qps else {
            return 0;
        };
        let diff = (1000 / qps) as i128 - average_duration;
        // 除数取整最大误差为1 忽略
        if diff <= 1 {
            0
        } else {
            diff as u64
        }
    }
}
