// Compile the shared gRPC contract (proto/inferer.proto) into Rust client and
// server stubs. The server side is generated too so the crate's tests can stand
// up an in-process mock Inferer and exercise the real transport end-to-end.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&["../../proto/inferer.proto"], &["../../proto"])?;
    println!("cargo:rerun-if-changed=../../proto/inferer.proto");
    Ok(())
}
