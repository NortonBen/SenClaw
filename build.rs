fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .compile_protos(&["proto/channel_relay.proto"], &["proto"])?;
    Ok(())
}
