fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    tonic_build::configure()
        .build_server(true)
        .compile_protos(&["proto/rsbox_api.proto"], &["proto"])?;
    Ok(())
}
