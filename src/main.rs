// use proglog_rs::storage::index::Index;
// use proglog_rs::storage::store::Store;
// use std::fs;
// use tempfile::TempDir;

use log::info;
use proglog_rs::server::grpc::{LogService, proto};
use proglog_rs::storage::log::{Log, LogConfig};
use proto::log_server::LogServer;
use std::fs::create_dir;
use std::path::PathBuf;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("starting proglog-rs gRPC server");

    let log_dir = PathBuf::from("data");
    create_dir(&log_dir)?;

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

// fn main() -> Result<(), Box<dyn std::error::Error>> {
//     // Initialize logging
//     tracing_subscriber::fmt().with_env_filter("debug").init();

//     println!("🚀 Testing Store + Index Integration");
//     println!("=====================================");

//     // Create temporary directory for our files
//     let temp_dir = TempDir::new()?;
//     let store_path = temp_dir.path().join("test.log");
//     let index_path = temp_dir.path().join("test.idx");

//     println!("📁 Store file: {store_path:?}");
//     println!("📁 Index file: {index_path:?}");
//     println!();

//     // Phase 1: Writing Records
//     println!("📝 Phase 1: Writing Records");
//     println!("---------------------------");

//     {
//         let mut store = Store::new(&store_path)?;
//         let mut index = Index::new(&index_path)?;

//         let records = [
//             "Hello, World!",
//             "This is record 2",
//             "Short",
//             "This is a much longer record with more text to see variable sizing",
//             "Final record",
//         ];

//         for (offset, record) in records.iter().enumerate() {
//             let data = record.as_bytes();

//             // Write to store first
//             let (position, bytes_written) = store.append(data)?;

//             // Then record the mapping in index
//             index.write(offset as u64, position)?;

//             println!(
//                 "✅ Record {offset}: '{record}' → Store position: {position}, Bytes written: {bytes_written}",
//             );
//         }

//         println!();
//         println!("📊 Final State:");
//         println!("   Store size: {} bytes", store.size());
//         println!("   Index entries: {}", index.len());
//     } // Store and Index are dropped here, flushing data

//     println!();

//     // Phase 2: Reading Records Back
//     println!("📖 Phase 2: Reading Records Back");
//     println!("---------------------------------");

//     {
//         // Reopen files to simulate persistence
//         let store = Store::new(&store_path)?;
//         let index = Index::new(&index_path)?;

//         println!("📂 Reopened files:");
//         println!("   Store size: {} bytes", store.size());
//         println!("   Index entries: {}", index.len());
//         println!();

//         // Read records by offset (using the index)
//         for offset in 0..index.len() {
//             // Look up position in index
//             let position = index.read(offset)?;

//             // Read actual data from store
//             let (data, bytes_read) = store.read(position)?;
//             let content = String::from_utf8_lossy(&data);

//             println!("🔍 Offset {offset} → Position {position} → '{content}' ({bytes_read} bytes)");
//         }
//     }

//     println!();

//     // Phase 3: Random Access Example
//     println!("🎯 Phase 3: Random Access");
//     println!("-------------------------");

//     {
//         let store = Store::new(&store_path)?;
//         let index = Index::new(&index_path)?;

//         // Read specific records out of order
//         let requests = [2, 0, 4, 1];

//         for &offset in &requests {
//             match index.read(offset) {
//                 Ok(position) => {
//                     let (data, _) = store.read(position)?;
//                     let content = String::from_utf8_lossy(&data);
//                     println!("📋 Requested offset {offset} → '{content}'");
//                 }
//                 Err(e) => {
//                     println!("❌ Error reading offset {offset}: {e}");
//                 }
//             }
//         }
//     }

//     println!();

//     // Phase 4: File Analysis
//     println!("🔬 Phase 4: File Analysis");
//     println!("-------------------------");

//     {
//         let store_metadata = fs::metadata(&store_path)?;
//         let index_metadata = fs::metadata(&index_path)?;

//         println!("📄 Store file: {} bytes", store_metadata.len());
//         println!("📄 Index file: {} bytes", index_metadata.len());

//         let index = Index::new(&index_path)?;
//         println!("📄 Index entries: {}", index.len());
//         println!(
//             "📄 Bytes per entry: {}",
//             index_metadata.len() / index.len().max(1)
//         );
//     }

//     println!();
//     println!("✨ All tests completed successfully!");

//     Ok(())
// }
