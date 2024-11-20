use crate::monitor::{Message, Monitor};
use clap::Parser;
use reqwest::{Client, ClientBuilder};
use std::{
    cmp, process,
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
    /// Timeout for each request
    #[arg(short, long)]
    timeout: Option<u64>,
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
        let mut client = ClientBuilder::new();
        if let Some(timeout) = args.timeout {
            client = client.timeout(Duration::from_secs(timeout));
        }
        let client = client.build().unwrap();
        Runner {
            now: Instant::now(),
            client: Arc::new(client),
            semaphore: Arc::new(Semaphore::new(args.max_concurrent)),
            request_count: Arc::new(AtomicU64::new(0)),
            average_duration: Arc::new(AtomicU64::new(0)),
            args,
        }
    }

    pub async fn run(&self) {
        let cancellation_token = CancellationToken::new();
        let mut handles = vec![];
        let (m_tx, m_rx) = mpsc::unbounded_channel::<Option<Message>>();
        let duration = self.args.duration;
        let max_count = self.args.n_requests;
        let mut monitor = Monitor::new(self.now.clone(), m_rx, max_count, duration);
        let inner_rx = monitor.get_receiver();
        // 执行monitor任务
        let m_handle = tokio::spawn(async move {
            monitor.run().await;
        });
        loop {
            // 结束时 取消其他任务
            if self.is_done() {
                cancellation_token.cancel();
                m_tx.send(None)
                    .unwrap_or_else(|_| eprintln!("send None failed"));
                break;
            }
            // 接收channel返回 原始终端只能通过monitor模块监听退出信号
            if *inner_rx.borrow() {
                cancellation_token.cancel();
                break;
            }
            let client = Arc::clone(&self.client);
            let semaphore = Arc::clone(&self.semaphore);
            let request_count = Arc::clone(&self.request_count);
            let average_duration = Arc::clone(&self.average_duration);
            let m_tx = m_tx.clone();
            let url = self.args.url.clone();
            let token = cancellation_token.clone();
            let diff = self.diff_qps_duration();
            // 限制请求频率
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
                        // 限制并发数
                        let _permit = semaphore.acquire().await.expect("semaphore acquire failed");
                        request_count.fetch_add(1, Ordering::SeqCst);
                        let start = Instant::now();
                        let is_success = client.get(url)
                            .send()
                            .await.is_ok();
                        let duration = start.elapsed().as_millis() as u64;
                        tx.send(duration).expect("send duration failed");
                        let mut average_duration = average_duration.load(Ordering::SeqCst);
                        if average_duration == 0 {
                            average_duration = duration;
                        }
                        m_tx.send(Some(Message{
                            duration,
                            is_success,
                            average_duration,
                        })).unwrap_or_else(|_| eprintln!("send Message failed"));
                        drop(_permit);
                    } => {}
                }
            });
            let duration = rx.recv().await.expect("get duration failed");
            self.set_average_duration(duration);
            handles.push(handle);
        }
        for handle in handles {
            handle.await.expect("Task panicked");
        }
        m_handle.await.expect("m_handle panicked");
        println!("Done! Duration: {:?}", self.now.elapsed());
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
        let average_duration = self.average_duration.load(Ordering::SeqCst);
        let request_count = self.request_count.load(Ordering::SeqCst);
        let average_all = average_duration
            / cmp::max(1, cmp::min(self.args.max_concurrent as u64, request_count));
        let Some(qps) = self.args.qps else {
            return 0;
        };
        let average_qps = 1000 / qps;
        let diff = average_qps.checked_sub(average_all).unwrap_or(0);
        // 除数取整最大误差为1 忽略
        if diff <= 1 {
            0
        } else {
            diff
        }
    }
}
