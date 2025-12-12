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

    println!("\n📖 Consuming records (random access - out of order)...");

    // Read in reverse order to demonstrate random access
    for &offset in offsets.iter().rev() {
        let request = tonic::Request::new(proto::ConsumeRequest { offset });
        let response = client.consume(request).await?;
        let inner = response.into_inner();
        let record = String::from_utf8_lossy(&inner.record);
        println!("  🔍 Offset {} → '{}'", inner.offset, record);
    }

    println!("\n📜 Sequential scan from offset 0...");

    // Demonstrate sequential scanning
    let mut offset = 0;
    loop {
        let request = tonic::Request::new(proto::ConsumeRequest { offset });
        match client.consume(request).await {
            Ok(response) => {
                let inner = response.into_inner();
                let record = String::from_utf8_lossy(&inner.record);
                println!("  📄 Offset {} → '{}'", inner.offset, record);
                offset += 1;
            }
            Err(_) => {
                break;
            }
        }
    }

    println!("\n✨ All operations completed successfully!");
    Ok(())
}
