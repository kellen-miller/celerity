use std::{collections::BTreeSet, process::Command};

#[test]
fn production_dependency_closure_excludes_authority_can_run_and_onnx() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(workspace)
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("metadata JSON");
    let packages = metadata["packages"].as_array().expect("packages");
    let viewer = packages
        .iter()
        .find(|package| package["name"] == "vehicle-ui")
        .expect("vehicle-ui package");
    let direct = viewer["dependencies"]
        .as_array()
        .expect("viewer dependencies")
        .iter()
        .filter(|dependency| dependency["kind"].is_null())
        .map(|dependency| dependency["name"].as_str().expect("dependency name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        direct,
        BTreeSet::from([
            "celerity-proto",
            "prost",
            "serde",
            "serde_json",
            "tiny_http",
            "vehicle-diagnostics",
        ])
    );

    let resolve = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes");
    let root_id = viewer["id"].as_str().expect("vehicle-ui ID");
    let mut pending = vec![root_id];
    let mut visited = BTreeSet::new();
    while let Some(package_id) = pending.pop() {
        if !visited.insert(package_id) {
            continue;
        }
        let node = resolve
            .iter()
            .find(|node| node["id"] == package_id)
            .expect("resolved package node");
        for dependency in node["deps"].as_array().expect("resolved dependencies") {
            let includes_production = dependency["dep_kinds"]
                .as_array()
                .expect("dependency kinds")
                .iter()
                .any(|kind| kind["kind"].is_null());
            if includes_production {
                pending.push(dependency["pkg"].as_str().expect("dependency package ID"));
            }
        }
    }
    let names = packages
        .iter()
        .filter(|package| visited.contains(package["id"].as_str().expect("resolved package ID")))
        .map(|package| package["name"].as_str().expect("resolved package name"))
        .collect::<BTreeSet<_>>();
    for forbidden in [
        "vehicle-runtime",
        "control-core",
        "control-protocol",
        "socketcan",
        "tract-core",
        "tract-onnx",
    ] {
        assert!(
            !names.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
    assert!(
        names.iter().all(|name| !name.starts_with("tract-")),
        "ONNX dependency present: {names:?}"
    );
}
