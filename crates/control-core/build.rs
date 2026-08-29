fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let mut configuration = prost_build::Config::new();
    configuration.protoc_executable(protoc);
    configuration.disable_comments(["."]);
    configuration.type_attribute(
        ".celerity.run.v1.RawCanFrame",
        "#[allow(clippy::struct_excessive_bools)]",
    );
    configuration
        .compile_protos(
            &["../../contracts/run/v1/run.proto"],
            &["../../contracts/run/v1"],
        )
        .expect("compile authoritative Run v1 contract");
    println!("cargo:rerun-if-changed=../../contracts/run/v1/run.proto");
}
