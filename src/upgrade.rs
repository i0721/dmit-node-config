use crate::store::Store;
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub fn run(store: &Store, version: &str, prefix: &Path, execute: bool) -> Result<()> {
    ensure!(prefix.is_absolute(), "Installation prefix must be absolute");
    let script = include_str!("../install.sh");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-s",
            "--",
            "--install-only",
            "--version",
            version,
            "--prefix",
        ])
        .arg(prefix)
        .arg("--config-dir")
        .arg(&store.config_dir)
        .arg("--state-dir")
        .arg(&store.state_dir)
        .arg(if execute { "--execute" } else { "--dry-run" })
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::piped());
    // The installer validates old inputs without switching services or snapshots.
    for name in [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "CURL_CA_BUNDLE",
        "SSL_CERT_FILE",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let _lock = if execute { Some(store.lock()?) } else { None };
    let mut child = command
        .spawn()
        .context("Cannot start trusted embedded installer")?;
    child
        .stdin
        .take()
        .context("Missing installer input")?
        .write_all(script.as_bytes())?;
    ensure!(
        child.wait()?.success(),
        "Upgrade failed; the installer preserves the original executable until verification succeeds"
    );
    Ok(())
}
