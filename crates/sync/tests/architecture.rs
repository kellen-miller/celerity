use std::process::Command;

use serde_json::Value;

#[test]
fn sync_has_no_authority_or_can_dependency_path() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata");
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("metadata JSON");
    let package = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|package| package["name"] == "sync")
        .expect("sync package");
    let forbidden = [
        "control-core",
        "control-protocol",
        "socketcan",
        "tokio-socketcan",
    ];
    for dependency in package["dependencies"].as_array().expect("dependencies") {
        if dependency["kind"].as_str() == Some("dev") {
            continue;
        }
        assert!(
            !forbidden.contains(&dependency["name"].as_str().expect("dependency name")),
            "sync dependency crosses the authority boundary: {dependency}"
        );
    }
}
