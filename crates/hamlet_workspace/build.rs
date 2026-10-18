fn main() -> Result<(), std::io::Error> {
    println!("cargo:rerun-if-changed=proto/ateapi.proto");
    let mut config = prost_build::Config::new();
    config.protoc_executable(
        protoc_bin_vendored::protoc_bin_path()
            .map_err(std::io::Error::other)?,
    );
    tonic_prost_build::configure()
        .build_server(false)
        .compile_with_config(
            config,
            &["proto/ateapi.proto"],
            &["proto"],
        )?;
    Ok(())
}
