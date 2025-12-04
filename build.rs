//! Build script for compiling Protocol Buffer schemas

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the log service proto
    let protos = &[
        "proto/log.proto",
        //    "proto/discovery.proto",
        //    "proto/raft.proto",
    ];
    // tonic_prost_build::compile_protos(protos, &["proto"])?;

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, &["proto"])?;

    Ok(())
}
