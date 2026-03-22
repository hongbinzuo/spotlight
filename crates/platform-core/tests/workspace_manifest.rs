use std::{fs, path::Path};

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("platform-core should live under the workspace root")
}

#[test]
fn root_workspace_manifest_tracks_foundation_members() {
    let manifest_path = workspace_root().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("should read workspace Cargo.toml");

    for expected in [
        "\"apps/server\"",
        "\"crates/platform-core\"",
        "\"crates/workflow-engine\"",
        "\"crates/provider-runtime\"",
        "\"crates/insight-engine\"",
    ] {
        assert!(
            manifest.contains(expected),
            "workspace manifest should include member {expected}"
        );
    }

    assert!(
        manifest.contains("\"apps/desktop/src-tauri\""),
        "desktop tauri crate should stay outside the 0.1.0 Rust workspace by default"
    );
    assert!(
        manifest.contains("version = \"0.1.0\""),
        "workspace package version should remain aligned with the 0.1.0 foundation slice"
    );
    assert!(
        manifest.contains("edition = \"2021\""),
        "workspace edition should be declared once at the workspace root"
    );
    assert!(
        manifest.contains("resolver = \"2\""),
        "workspace should keep the modern feature resolver enabled"
    );
}

#[test]
fn server_manifest_consumes_workspace_settings_and_local_foundation_crates() {
    let manifest_path = workspace_root().join("apps/server/Cargo.toml");
    let manifest =
        fs::read_to_string(&manifest_path).expect("should read spotlight-server Cargo.toml");

    for expected in [
        "version.workspace = true",
        "edition.workspace = true",
        "license.workspace = true",
        "platform-core = { path = \"../../crates/platform-core\" }",
        "provider-runtime = { path = \"../../crates/provider-runtime\" }",
    ] {
        assert!(
            manifest.contains(expected),
            "server manifest should contain {expected}"
        );
    }
}
