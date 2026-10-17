use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path, process::Command};
fn command(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .envs(
            [
                "HTTPS_PROXY",
                "HTTP_PROXY",
                "NO_PROXY",
                "https_proxy",
                "http_proxy",
                "no_proxy",
            ]
            .into_iter()
            .filter_map(|key| std::env::var_os(key).map(|value| (key, value))),
        )
        .output()?;
    ensure!(
        output.status.success(),
        "Artifact tool failed: {program}; output withheld"
    );
    Ok(output.stdout)
}
pub fn download_xray(output: &Path) -> Result<()> {
    let locks: toml::Value = toml::from_str(include_str!("../versions.lock.toml"))?;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => bail!("Unsupported Xray architecture"),
    };
    ensure!(
        cfg!(target_os = "linux"),
        "Xray helper installs Linux binaries only"
    );
    let lock = &locks["xray"];
    let url = lock[format!("linux_{arch}_url")]
        .as_str()
        .context("Xray URL")?;
    let work = tempfile::tempdir()?;
    let archive = work.path().join("xray.zip");
    command(
        "curl",
        &[
            "--disable",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "180",
            "-o",
            archive.to_str().context("archive path")?,
            url,
        ],
    )?;
    ensure!(
        hex::encode(Sha256::digest(fs::read(&archive)?))
            == lock[format!("linux_{arch}_archive_sha256")]
                .as_str()
                .context("Xray digest")?,
        "Xray archive digest mismatch"
    );
    let binary = command("unzip", &["-p", archive.to_str().unwrap(), "xray"])?;
    ensure!(!binary.is_empty(), "Empty Xray archive");
    let mut staged = tempfile::NamedTempFile::new_in(output.parent().context("Output parent")?)?;
    staged.write_all(&binary)?;
    staged.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    staged.persist(output)?;
    println!(
        "Verified official Xray {} {arch}",
        lock["version"].as_str().unwrap()
    );
    Ok(())
}
