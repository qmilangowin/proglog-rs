use proglog_rs::server::grpc::proto::{self, log_client::LogClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = LogClient::connect("http://[::1]:50051").await?;

    println!("🔌 Connected to server");

    // produce some records
    println!("\n📝 Producing records...");

    let records = vec![
        "Hello, gRPC!",
        "This is record 2",
        "Testing the distributed log",
        "Fourth record here",
    ];

    let mut offsets = Vec::new();
    for record in &records {
        let request = tonic::Request::new(proto::ProduceRequest {
            record: record.as_bytes().to_vec(),
        });

        let response = client.produce(request).await?;
        let offset = response.into_inner().offset;
        offsets.push(offset);

        println!("  ✅ Produced: '{}' → offset {}", record, offset);
    }

    println!("\n📖 Consuming records...");

    for offset in offsets {
        let request = tonic::Request::new(proto::ConsumeRequest { offset });
        let response = client.consume(request).await?;
        let inner = response.into_inner();
        let record = String::from_utf8_lossy(&inner.record);
        println!("  🔍 Offset {} → '{}'", inner.offset, record);
    }

    println!("\n✨ All operations completed successfully!");
    Ok(())
}
