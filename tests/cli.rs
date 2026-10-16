#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

fn invoke(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nodectl"))
        .arg("--config-dir")
        .arg(root.join("config"))
        .arg("--state-dir")
        .arg(root.join("state"))
        .args(args)
        .output()
        .unwrap()
}
fn setup_args(xray: &Path) -> Vec<String> {
    vec![
        "setup",
        "--domain",
        "node.example.com",
        "--reality-domain",
        "reality.example.com",
        "--ipv4",
        "192.0.2.10",
        "--non-interactive",
        "--xray-binary",
        xray.to_str().unwrap(),
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
#[test]
fn preview_is_read_only_and_execute_is_atomic_and_refuses_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let fake = root.join("xray");
    // Only key generation is stubbed; no services or deployment are simulated.
    fs::write(
        &fake,
        "#!/bin/sh\nprintf 'PrivateKey: BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let mut args = setup_args(&fake);
    let output = invoke(root, &args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join("config").exists() && !root.join("state").exists());
    args.push("--execute".into());
    let output = invoke(root, &args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let original = fs::read(root.join("config/secrets.json")).unwrap();
    assert_eq!(
        fs::metadata(root.join("config"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.join("config/secrets.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        !invoke(root, &args.iter().map(String::as_str).collect::<Vec<_>>())
            .status
            .success()
    );
    assert_eq!(
        original,
        fs::read(root.join("config/secrets.json")).unwrap()
    );
    assert!(invoke(root, &["migrate"]).status.success());
    assert_eq!(
        original,
        fs::read(root.join("config/secrets.json")).unwrap()
    );
}
#[test]
fn invalid_or_failed_setup_never_leaves_partial_configuration() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nonexistent-xray");
    let mut args = setup_args(&missing);
    args.push("--execute".into());
    assert!(
        !invoke(
            tmp.path(),
            &args.iter().map(String::as_str).collect::<Vec<_>>()
        )
        .status
        .success()
    );
    assert!(!tmp.path().join("config").exists());
}
#[test]
fn doctor_aggregates_errors_and_status_handles_uninitialized_host_without_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let doctor = invoke(tmp.path(), &["doctor", "--json"]);
    assert!(!doctor.status.success());
    let data: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert!(data["checks"].as_array().unwrap().len() >= 7);
    let status = invoke(tmp.path(), &["status", "--json"]);
    assert!(status.status.success());
    let data: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(data["current"].is_null());
    assert!(!tmp.path().join("config").exists() && !tmp.path().join("state").exists());
}
#[test]
fn upgrade_preview_does_not_download_or_create_state() {
    let tmp = tempfile::tempdir().unwrap();
    let output = invoke(tmp.path(), &["upgrade", "--version", "v0.2.0"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!tmp.path().join("state").exists());
    assert!(
        !invoke(tmp.path(), &["upgrade", "--version", "../../evil"])
            .status
            .success()
    );
}
