use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PROTO_RELATIVE_PATH: &str = "contracts/celerity/v1/celerity.proto";
const PROTO_INPUT: &str = "celerity/v1/celerity.proto";

fn main() -> ExitCode {
    if let Err(error) = generate() {
        eprintln!("proto generation failed: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn generate() -> Result<(), String> {
    let repository_root = repository_root();
    let output_root = output_root(&repository_root)?;
    let proto_path = repository_root.join(PROTO_RELATIVE_PATH);
    let contracts_root = repository_root.join("contracts");

    if !proto_path.is_file() {
        return Err(format!(
            "missing authoritative proto: {}",
            proto_path.display()
        ));
    }

    generate_rust(&proto_path, &contracts_root, &output_root)?;
    generate_python(&repository_root, &contracts_root, &output_root)?;
    generate_typescript(&repository_root, &contracts_root, &output_root)?;
    generate_python_package_markers(&output_root)?;

    Ok(())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("proto-codegen must live under crates/")
        .to_path_buf()
}

fn output_root(repository_root: &Path) -> Result<PathBuf, String> {
    let mut args = env::args_os().skip(1);
    let mut output_root = repository_root.to_path_buf();

    while let Some(argument) = args.next() {
        if argument == "--output-root" {
            let value = args
                .next()
                .ok_or_else(|| "--output-root requires a path".to_owned())?;
            output_root = PathBuf::from(value);
        } else {
            return Err(format!(
                "unexpected argument: {}",
                argument.to_string_lossy()
            ));
        }
    }

    Ok(output_root)
}

fn generate_rust(
    proto_path: &Path,
    contracts_root: &Path,
    output_root: &Path,
) -> Result<(), String> {
    let output_directory = output_root.join("crates/celerity-proto/src/generated");
    fs::create_dir_all(&output_directory).map_err(|error| error.to_string())?;

    let protoc = protoc_bin_vendored::protoc_bin_path().map_err(|error| error.to_string())?;
    let mut configuration = prost_build::Config::new();
    configuration.out_dir(&output_directory);
    configuration.protoc_executable(&protoc);
    configuration.disable_comments(["."]);
    configuration.type_attribute(
        "celerity.v1.RawCanFrame",
        "#[allow(clippy::struct_excessive_bools)]",
    );
    configuration
        .compile_protos(&[proto_path], &[contracts_root])
        .map_err(|error| error.to_string())
}

fn generate_python(
    repository_root: &Path,
    contracts_root: &Path,
    output_root: &Path,
) -> Result<(), String> {
    let output_directory = output_root.join("services/home/src");
    fs::create_dir_all(&output_directory).map_err(|error| error.to_string())?;
    let protoc = protoc_bin_vendored::protoc_bin_path().map_err(|error| error.to_string())?;

    run_protoc(
        &protoc,
        repository_root,
        [
            OsString::from(format!("--proto_path={}", contracts_root.display())),
            OsString::from(format!("--python_out={}", output_directory.display())),
            OsString::from(format!("--pyi_out={}", output_directory.display())),
            OsString::from(PROTO_INPUT),
        ],
    )
}

fn generate_typescript(
    repository_root: &Path,
    contracts_root: &Path,
    output_root: &Path,
) -> Result<(), String> {
    let output_directory = output_root.join("crates/vehicle-ui/web/src/generated");
    fs::create_dir_all(&output_directory).map_err(|error| error.to_string())?;
    let protoc = protoc_bin_vendored::protoc_bin_path().map_err(|error| error.to_string())?;
    let plugin =
        repository_root.join("crates/vehicle-ui/web/node_modules/.bin/protoc-gen-ts_proto");

    if !plugin.is_file() {
        return Err(format!(
            "missing ts-proto plugin: {}; run npm ci in crates/vehicle-ui/web",
            plugin.display()
        ));
    }

    run_protoc(
        &protoc,
        repository_root,
        [
            OsString::from(format!("--proto_path={}", contracts_root.display())),
            OsString::from(format!("--plugin=protoc-gen-ts_proto={}", plugin.display())),
            OsString::from(format!("--ts_proto_out={}", output_directory.display())),
            OsString::from(
                "--ts_proto_opt=esModuleInterop=true,forceLong=number,useOptionals=messages,outputJsonMethods=true,outputEncodeMethods=true,outputDecodeMethods=true,outputPartialMethods=true,snakeToCamel=none,stringEnums=true,env=browser",
            ),
            OsString::from(PROTO_INPUT),
        ],
    )
}

fn run_protoc<I>(protoc: &Path, working_directory: &Path, arguments: I) -> Result<(), String>
where
    I: IntoIterator<Item = OsString>,
{
    let output = Command::new(protoc)
        .current_dir(working_directory)
        .args(arguments)
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

fn generate_python_package_markers(output_root: &Path) -> Result<(), String> {
    for directory in [
        output_root.join("services/home/src/celerity"),
        output_root.join("services/home/src/celerity/v1"),
    ] {
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        fs::write(
            directory.join("__init__.py"),
            "# Generated package marker; do not edit.\n",
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}
