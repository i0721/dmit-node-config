//! Access credentials are independent of release snapshots: rollback must never
//! resurrect a revoked credential. Never derive Debug or log request values.
use crate::store::{self, Store};
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
use subtle::ConstantTimeEq;

#[derive(Subcommand)]
pub enum Action {
    /// Serve only the protected Unix socket used by Nginx.
    Serve {
        /// Infer the protected store from the verified managed unit symlink.
        #[arg(long, hide = true)]
        from_systemd: bool,
    },
    /// Show credential status and its protected path, without printing it.
    Status,
    /// Show the saved access token only on the controlling terminal.
    Reveal,
    /// Revoke the previous token and save a new one (random by default).
    Rotate {
        /// Read a custom token from an owner-only file; never pass its value.
        #[arg(long, conflicts_with = "prompt")]
        token_file: Option<PathBuf>,
        /// Read a custom token without terminal echo.
        #[arg(long)]
        prompt: bool,
    },
    /// Immediately refuse all existing subscription credentials.
    Revoke,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    schema_version: u32,
    generation: u64,
    revoked: bool,
    token: String,
    sha256: String,
    #[serde(default)]
    displayed: bool,
}

pub fn credential_path(store: &Store) -> PathBuf {
    store.state_dir.join("distribution/access.json")
}

pub(crate) fn access_status(store: &Store) -> &'static str {
    if !credential_path(store).exists() {
        return "not-configured";
    }
    match read(store) {
        Ok(record) if record.revoked => "revoked",
        Ok(_) => "active",
        Err(_) => "invalid",
    }
}

fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && token
            .bytes()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 8
}

pub(crate) fn validate_custom(token: &str) -> Result<()> {
    ensure!(
        valid_token(token),
        "Access token must be 64 lowercase hexadecimal characters with at least eight distinct characters; use a cryptographic random source"
    );
    Ok(())
}

fn read(store: &Store) -> Result<Credential> {
    store::check_private(&store.state_dir, true)?;
    let path = credential_path(store);
    store::check_private(path.parent().unwrap(), true)?;
    store::check_private(&path, false)?;
    ensure!(
        fs::metadata(&path)?.len() < 4096,
        "Invalid access credential record"
    );
    // Deserialize failures must not quote raw credential data in diagnostics.
    let record: Credential = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| anyhow::anyhow!("Invalid access credential record"))?;
    ensure!(
        record.schema_version == 1
            && record.generation > 0
            && valid_token(&record.token)
            && record.sha256 == store::digest(record.token.as_bytes()),
        "Invalid access credential record"
    );
    Ok(record)
}

pub fn rotate(store: &Store, custom: Option<String>) -> Result<()> {
    let path = credential_path(store);
    let generation = if path.exists() {
        read(store)?
            .generation
            .checked_add(1)
            .context("Credential generation exhausted")?
    } else {
        1
    };
    let token = match custom {
        Some(token) => token,
        None => {
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        }
    };
    ensure!(
        valid_token(&token),
        "Access token must be 64 lowercase hexadecimal characters with at least eight distinct characters; use a cryptographic random source"
    );
    if path.exists() {
        ensure!(
            read(store)?.sha256 != store::digest(token.as_bytes()),
            "A rotated token must differ from the previous token"
        );
    }
    store::private_dir(path.parent().unwrap())?;
    store::atomic_write(
        &path,
        &serde_json::to_vec_pretty(&Credential {
            schema_version: 1,
            generation,
            revoked: false,
            sha256: store::digest(token.as_bytes()),
            token,
            displayed: false,
        })?,
    )
}

pub fn revoke(store: &Store) -> Result<()> {
    if !credential_path(store).exists() {
        return Ok(());
    }
    let mut record = read(store)?;
    record.revoked = true;
    store::atomic_write(
        &credential_path(store),
        &serde_json::to_vec_pretty(&record)?,
    )
}

pub fn authorized(store: &Store, token: &str) -> bool {
    if !valid_token(token) {
        return false;
    }
    let Ok(record) = read(store) else {
        return false;
    };
    !record.revoked
        && bool::from(
            record
                .sha256
                .as_bytes()
                .ct_eq(store::digest(token.as_bytes()).as_bytes()),
        )
}

fn reveal(store: &Store) -> Result<()> {
    let record = read(store)?;
    ensure!(
        !record.revoked,
        "Access is revoked; use nodectl share rotate to restore it"
    );
    #[cfg(unix)]
    {
        // A redirected stdout, CI log or deployment report never receives this.
        let mut tty = fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty")
            .context(
                "A controlling terminal is required; read the protected credential file instead",
            )?;
        writeln!(tty, "Access token (keep private): {}", record.token)?;
        if let Some(id) = store.pointer("current")?
            && let Ok((c, _)) = store.verify(&id)
            && c.distributes()
        {
            let port = if c.ports.https == 443 {
                String::new()
            } else {
                format!(":{}", c.ports.https)
            };
            for format in ["mihomo", "xray"] {
                writeln!(
                    tty,
                    "Private {format} subscription: https://{}{port}/nodes/sub/{format}?token={}",
                    c.node.domain, record.token
                )?;
            }
        }
        tty.flush()?;
        Ok(())
    }
    #[cfg(not(unix))]
    anyhow::bail!("Access token display requires a Linux controlling terminal")
}

pub(crate) fn deployment_result(store: &Store) -> Result<()> {
    let id = store
        .pointer("current")?
        .context("No successful deployment")?;
    let (c, _) = store.verify(&id)?;
    println!(
        "Successful deployment: {id}\nServices: protocol health checks passed\nManage: nodectl status | nodectl doctor | nodectl rollback"
    );
    crate::web_services::deployment_result(&c, true)?;
    if !c.distributes() {
        return Ok(());
    }
    let origin = if c.ports.https == 443 {
        format!("https://{}", c.node.domain)
    } else {
        format!("https://{}:{}", c.node.domain, c.ports.https)
    };
    println!(
        "Node page: {origin}/nodes/\nSubscription templates (replace locally; keep private):\n  {origin}/nodes/sub/mihomo?token=<ACCESS_TOKEN>\n  {origin}/nodes/sub/xray?token=<ACCESS_TOKEN>\nFormats: Mihomo provider and Xray outbound profiles\nProtected access credential: {}\nRecover access: nodectl share reveal\nRotate: nodectl share rotate\nRevoke: nodectl share revoke",
        credential_path(store).display()
    );
    let mut record = read(store)?;
    if record.revoked {
        println!("Distribution access remains revoked; explicitly rotate to restore access.");
    } else if !record.displayed && reveal(store).is_ok() {
        record.displayed = true;
        store::atomic_write(
            &credential_path(store),
            &serde_json::to_vec_pretty(&record)?,
        )?;
    }
    Ok(())
}

pub fn run(store: &Store, action: Action) -> Result<()> {
    match action {
        Action::Serve { from_systemd } => {
            if from_systemd {
                crate::host::linux_root()?;
                let target = fs::read_link("/etc/systemd/system/nodectl-share.service")?;
                let release = target
                    .parent()
                    .and_then(|p| p.parent())
                    .context("Invalid service target")?;
                let state = release
                    .parent()
                    .and_then(|p| p.parent())
                    .context("Invalid service store")?;
                let id = release
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("Invalid service release")?;
                let discovered = Store::new(release.join("source"), state.into())?;
                ensure!(
                    target
                        == discovered
                            .release(id)?
                            .join("systemd/nodectl-share.service"),
                    "Invalid service target"
                );
                discovered.verify(id)?;
                return crate::share_http::serve(&discovered);
            }
            return crate::share_http::serve(store);
        }
        Action::Status => {
            let record = read(store)?;
            println!(
                "Access: {}\nGeneration: {}\nProtected credential: {}",
                if record.revoked { "revoked" } else { "active" },
                record.generation,
                credential_path(store).display()
            );
        }
        Action::Reveal => {
            let _guard = store.lock()?;
            reveal(store)?;
        }
        Action::Rotate { token_file, prompt } => {
            let _guard = store.lock()?;
            let id = store
                .pointer("current")?
                .context("Deploy enabled distribution before rotating access")?;
            ensure!(
                store.verify(&id)?.0.distributes(),
                "Enable distribution and deploy before rotating access"
            );
            ensure!(
                fs::read_link("/etc/nginx/nginx.conf").ok()
                    == Some(store.release(&id)?.join("nginx.conf")),
                "Resolve Nginx entrypoint drift before rotating access"
            );
            let custom = if let Some(path) = token_file {
                store::check_private(&path, false)?;
                ensure!(fs::metadata(&path)?.len() <= 128, "Invalid token file");
                Some(fs::read_to_string(path)?.trim().to_owned())
            } else if prompt {
                Some(crate::deploy::secret_prompt(
                    "New access token (64 lowercase hex characters)",
                )?)
            } else {
                None
            };
            rotate(store, custom)?;
            println!(
                "Previous access revoked. Protected credential: {}\nUse nodectl share reveal on a private terminal.",
                credential_path(store).display()
            );
        }
        Action::Revoke => {
            let _guard = store.lock()?;
            revoke(store)?;
            println!("Access revoked. Use nodectl share rotate to restore access.");
        }
    }
    Ok(())
}

/// Caller must hold a shared store lock until the response is sent. This check
/// deliberately uses success, not checks: a checked candidate is not deployed.
pub fn current_clients(store: &Store, format: &str) -> Result<(String, Vec<u8>)> {
    for journal in ["deployment.json", "transaction.json", "uninstall.json"] {
        ensure!(
            !store.state_dir.join(journal).exists(),
            "Deployment is changing; retry later"
        );
    }
    let id = store
        .pointer("current")?
        .context("No successful deployment")?;
    let success = store.state_dir.join("success").join(&id);
    store::check_private(success.parent().unwrap(), true)?;
    store::check_private(&success, false)?;
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(success)?)?;
    ensure!(
        receipt["release"] == id,
        "Successful deployment receipt mismatch"
    );
    let (c, s) = store.verify(&id)?;
    ensure!(
        c.distributes(),
        "Distribution is disabled for this successful deployment"
    );
    let bytes = match format {
        "xray" => fs::read(store.release(&id)?.join("clients/profiles.json"))?,
        "mihomo" => crate::render::mihomo(&c, &s)?,
        _ => anyhow::bail!("Unsupported client format"),
    };
    ensure!(bytes.len() <= 1024 * 1024, "Client response too large");
    Ok((id, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sandbox() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().join("config"), tmp.path().join("state")).unwrap();
        store::private_dir(&store.state_dir).unwrap();
        (tmp, store)
    }
    #[test]
    fn random_rotation_revocation_and_recovery_never_reuse_credentials() {
        let (_tmp, store) = sandbox();
        assert!(!authorized(&store, &"1".repeat(64)));
        rotate(&store, None).unwrap();
        let first = read(&store).unwrap().token;
        assert!(authorized(&store, &first));
        assert!(!authorized(&store, "incorrect"));
        rotate(&store, None).unwrap();
        let second = read(&store).unwrap().token;
        assert_ne!(first, second);
        assert!(!authorized(&store, &first));
        assert!(authorized(&store, &second));
        revoke(&store).unwrap();
        revoke(&store).unwrap();
        assert!(!authorized(&store, &second));
        assert!(reveal(&store).is_err());
        rotate(&store, None).unwrap();
        assert_eq!(read(&store).unwrap().generation, 3);
        assert!(!authorized(&store, &second));
        store::check_private(&credential_path(&store), false).unwrap();
    }
    #[test]
    fn malformed_or_weak_custom_tokens_and_tampered_records_fail_closed() {
        let (_tmp, store) = sandbox();
        for token in ["", "short", &"a".repeat(64), &"Z".repeat(64)] {
            assert!(rotate(&store, Some(token.into())).is_err());
        }
        let custom = "0123456789abcdef".repeat(4);
        rotate(&store, Some(custom.clone())).unwrap();
        assert!(authorized(&store, &custom));
        let path = credential_path(&store);
        let mut record = read(&store).unwrap();
        record.sha256 = "f".repeat(64);
        store::atomic_write(&path, &serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(!authorized(&store, &custom));
        assert!(rotate(&store, None).is_err());
        store::atomic_write(&path, b"not json").unwrap();
        assert!(!authorized(&store, &custom));
    }
    #[test]
    fn interrupted_deployments_never_expose_candidate_clients() {
        let (_tmp, store) = sandbox();
        assert!(current_clients(&store, "xray").is_err());
        for journal in ["deployment.json", "transaction.json", "uninstall.json"] {
            store::atomic_write(&store.state_dir.join(journal), b"{}").unwrap();
            assert!(current_clients(&store, "xray").is_err());
            fs::remove_file(store.state_dir.join(journal)).unwrap();
        }
    }
}
