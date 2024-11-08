mod runner;

use clap::Parser;
use runner::{Args, Runner};

// qps
// tui

#[tokio::main]
async fn main() {
    let args = Args::parse();
    println!("{:?}", args);
    Runner::new(args).run().await;
}
