// use proglog_rs::storage::index::Index;
// use proglog_rs::storage::store::Store;
// use std::fs;
// use tempfile::TempDir;

use log::info;
use proglog_rs::server::grpc::{LogService, proto};
use proglog_rs::storage::log::{Log, LogConfig};
use proto::log_server::LogServer;
use std::fs::create_dir_all;
use std::path::PathBuf;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("starting proglog-rs gRPC server");

    let log_dir = PathBuf::from("data");
    create_dir_all(&log_dir)?;

    let config = LogConfig {
        max_store_bytes: 1024 * 1024,
        max_index_entries: 1000,
        log_dir: log_dir.clone(),
    };

    let prog_log = Log::new(config)?;

    info!("Log initialized in ./data directory");

    let log_service = LogService::new(prog_log);

    let addr = "[::1]:50051".parse()?;
    info!("Server listening on {addr}");

    Server::builder()
        .add_service(LogServer::new(log_service))
        .serve(addr)
        .await?;
    Ok(())
}
