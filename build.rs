fn main() -> Result<(), Box<dyn std::error::Error>> {
    const VENDOR_ROOT: &str = "proto/vendor/authzed-api";
    let protos = [
        "proto/vendor/authzed-api/authzed/api/v1/permission_service.proto",
        "proto/vendor/authzed-api/authzed/api/v1/schema_service.proto",
    ];

    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);

    tonic_prost_build::configure()
        .build_server(false)
        .include_file("spicedb.rs")
        .compile_protos(&protos, &[VENDOR_ROOT])?;

    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }
    println!("cargo:rerun-if-changed={VENDOR_ROOT}/authzed/api/v1/core.proto");
    println!("cargo:rerun-if-changed={VENDOR_ROOT}/authzed/api/v1/debug.proto");
    println!("cargo:rerun-if-changed={VENDOR_ROOT}/PIN.md");

    Ok(())
}
