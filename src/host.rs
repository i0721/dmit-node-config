use crate::{
    config::{Config, Secrets},
    render,
    store::{self, Store},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// Tool output can contain private keys or full configurations. Never put raw
// stderr/stdout in an error or ordinary log. Files avoid pipe-buffer deadlocks.
pub fn command(binary: &str, args: &[&str]) -> Result<String> {
    String::from_utf8(run(binary, args, None)?).context("Tool returned invalid UTF-8")
}
fn run(binary: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    run_capture(binary, args, input, false)
}
pub(crate) fn command_stderr(binary: &str, args: &[&str]) -> Result<String> {
    String::from_utf8(run_capture(binary, args, None, true)?).context("Tool returned invalid UTF-8")
}
fn run_capture(
    binary: &str,
    args: &[&str],
    input: Option<&[u8]>,
    capture_stderr: bool,
) -> Result<Vec<u8>> {
    let out = tempfile::tempfile()?;
    let err = tempfile::tempfile()?;
    let mut stdin = tempfile::tempfile()?;
    if let Some(data) = input {
        stdin.write_all(data)?;
    }
    use std::io::{Seek, SeekFrom};
    stdin.seek(SeekFrom::Start(0))?;
    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(stdin)
        .stdout(out.try_clone()?)
        .stderr(if capture_stderr {
            out.try_clone()?
        } else {
            err
        })
        .spawn()
        .with_context(|| format!("Cannot start tool {binary}"))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Tool timed out: {binary}");
        }
        thread::sleep(Duration::from_millis(30));
    };
    ensure!(
        status.success(),
        "Tool failed: {binary} (exit {}; output withheld because it may contain credentials)",
        status.code().unwrap_or(-1)
    );
    let mut out = out;
    out.seek(SeekFrom::Start(0))?;
    let mut result = vec![];
    out.take(4 * 1024 * 1024).read_to_end(&mut result)?;
    Ok(result)
}
fn linux_root() -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "Host operations require Linux with systemd"
    );
    #[cfg(unix)]
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "Host operations require root"
    );
    Ok(())
}
fn path(p: &Path) -> Result<&str> {
    p.to_str().context("Non UTF-8 path")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub binary: PathBuf,
    pub sha256: String,
    pub version: String,
    pub probe_url: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    release: String,
    reference: Reference,
    dependencies: BTreeMap<String, String>,
}

impl Reference {
    fn verify(&self) -> Result<()> {
        ensure!(
            self.binary.is_absolute()
                && self.sha256.len() == 64
                && hex::decode(&self.sha256).is_ok(),
            "Reference client requires an absolute path and SHA256"
        );
        ensure!(
            !self.version.is_empty()
                && self
                    .version
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.'),
            "Reference version must be an exact dotted version"
        );
        ensure!(
            self.probe_url.starts_with("https://")
                && self.probe_url.len() > 8
                && !self.probe_url.chars().any(char::is_whitespace),
            "Handshake probe requires an HTTPS URL"
        );
        trusted_binary(&self.binary)?;
        ensure!(
            store::file_digest(&self.binary)? == self.sha256.to_ascii_lowercase(),
            "Reference client digest mismatch"
        );
        let version = command(path(&self.binary)?, &["version"])?;
        ensure!(
            version.split_whitespace().any(|s| s == self.version),
            "Reference client version mismatch"
        );
        Ok(())
    }
}

fn trusted_binary(p: &Path) -> Result<()> {
    let m = fs::metadata(p)?;
    ensure!(m.is_file(), "Missing executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            m.uid() == 0 && m.mode() & 0o022 == 0 && m.mode() & 0o111 != 0,
            "Executable must be root-owned and not writable by group/others: {}",
            p.display()
        );
    }
    Ok(())
}

fn dependencies(c: &Config, reference: &Reference) -> Result<BTreeMap<String, String>> {
    let mut files = vec![
        c.runtime.xray_binary.clone(),
        c.runtime.nginx_binary.clone(),
        path(&reference.binary)?.to_owned(),
    ];
    for dir in [&c.tls.domain_cert_dir, &c.tls.ip_cert_dir] {
        for file in ["fullchain.pem", "privkey.pem"] {
            files.push(format!("{dir}/{file}"));
        }
    }
    if c.warp.enabled {
        files.extend([c.runtime.usque_binary.clone(), c.warp.config_file.clone()]);
    }
    files
        .into_iter()
        .map(|p| Ok((p.clone(), store::file_digest(Path::new(&p))?)))
        .collect()
}

pub(crate) fn certificates(c: &Config) -> Result<()> {
    for (dir, sans, is_ip) in [
        (&c.tls.domain_cert_dir, c.domains(), false),
        (&c.tls.ip_cert_dir, c.ips(), true),
    ] {
        let cert = format!("{dir}/fullchain.pem");
        let key = format!("{dir}/privkey.pem");
        store::check_private(Path::new(&key), false)?;
        command(
            "openssl",
            &["x509", "-in", &cert, "-noout", "-checkend", "86400"],
        )?;
        command(
            "openssl",
            &[
                "verify",
                "-CApath",
                "/etc/ssl/certs",
                "-untrusted",
                &cert,
                &cert,
            ],
        )?;
        let san = command(
            "openssl",
            &["x509", "-in", &cert, "-noout", "-ext", "subjectAltName"],
        )?;
        let entries = san
            .lines()
            .skip(1)
            .flat_map(|l| l.split(','))
            .map(str::trim)
            .collect::<Vec<_>>();
        for name in sans {
            let covered = if is_ip {
                entries
                    .iter()
                    .filter_map(|s| s.strip_prefix("IP Address:"))
                    .any(|v| {
                        v.parse::<std::net::IpAddr>().ok() == name.parse::<std::net::IpAddr>().ok()
                    })
            } else {
                entries.contains(&format!("DNS:{name}").as_str())
            };
            ensure!(
                covered,
                "Certificate missing a required {} SAN",
                if is_ip { "IP" } else { "DNS" }
            );
        }
        if !is_ip {
            let text = command("openssl", &["x509", "-in", &cert, "-noout", "-text"])?;
            ensure!(
                text.contains("secp384r1"),
                "Domain certificate must use ECDSA P-384"
            );
        }
        let cert_pub = run(
            "openssl",
            &["x509", "-in", &cert, "-noout", "-pubkey"],
            None,
        )?;
        let cert_der = run(
            "openssl",
            &["pkey", "-pubin", "-outform", "DER"],
            Some(&cert_pub),
        )?;
        let key_der = run(
            "openssl",
            &["pkey", "-in", &key, "-pubout", "-outform", "DER"],
            None,
        )?;
        ensure!(
            cert_der == key_der,
            "Certificate and private key do not match"
        );
    }
    Ok(())
}

fn access(user: &str, group: Option<&str>, flag: &str, file: &str) -> Result<()> {
    let mut args = vec!["-u", user];
    if let Some(group) = group {
        args.extend(["-g", group]);
    }
    args.extend(["--", "test", flag, file]);
    command("runuser", &args)
        .map(|_| ())
        .with_context(|| format!("Service-user permission check failed ({user}, {flag})"))
}

fn preflight(store: &Store, id: &str, reference: &Reference) -> Result<(Config, Secrets)> {
    linux_root()?;
    let (c, s) = store.verify(id)?;
    reference.verify()?;
    for binary in [&c.runtime.xray_binary, &c.runtime.nginx_binary] {
        trusted_binary(Path::new(binary))?;
    }
    let lock: toml::Value = toml::from_str(include_str!("../versions.lock.toml"))?;
    let expected = lock["xray"]["version"]
        .as_str()
        .context("Invalid Xray version lock")?;
    ensure!(
        command(&c.runtime.xray_binary, &["version"])?
            .split_whitespace()
            .any(|v| v == expected),
        "Installed Xray does not match versions.lock.toml"
    );
    ensure!(
        c.domains()
            .iter()
            .all(|d| d != "example.com" && !d.ends_with(".example.com"))
            && !c.node.ipv4.starts_with("203.0.113.")
            && !c.node.ipv4.starts_with("192.0.2.")
            && !c.node.ipv4.starts_with("198.51.100.")
            && !c.node.ipv6.to_ascii_lowercase().starts_with("2001:db8:"),
        "Replace documentation domains and IP addresses before deployment"
    );
    command("id", &[&c.runtime.xray_user])?;
    command("id", &[&c.runtime.nginx_user])?;
    command("getent", &["group", &c.runtime.nginx_user])?;
    certificates(&c)?;
    for route in [&c.transport.xhttp.path, &c.transport.ws.path] {
        ensure!(
            !Path::new(&c.website.root)
                .join(route.trim_start_matches('/'))
                .exists(),
            "Transport route masks an existing website path"
        );
    }
    access(&c.runtime.nginx_user, None, "-r", &c.website.root)?;
    access(&c.runtime.nginx_user, None, "-x", &c.website.root)?;
    access(
        &c.runtime.xray_user,
        Some(&c.runtime.nginx_user),
        "-w",
        &format!("{}/xray", c.runtime.log_root),
    )?;
    for socket in [&c.transport.xhttp.socket, &c.transport.ws.socket] {
        let parent = Path::new(socket).parent().unwrap();
        access(
            &c.runtime.xray_user,
            Some(&c.runtime.nginx_user),
            "-w",
            path(parent)?,
        )?;
        access(&c.runtime.nginx_user, None, "-x", path(parent)?)?;
    }
    let release = store.release(id)?;
    // systemd credentials give the unprivileged process access without opening
    // the root-only snapshot. Test only as the service user: even -test opens
    // log files, so a root test would create files the service cannot write.
    let tmp = tempfile::Builder::new()
        .prefix("nodectl-check-")
        .tempdir_in("/run")?;
    let candidate = tmp.path().join("xray.json");
    store::atomic_write(&candidate, &fs::read(release.join("xray.json"))?)?;
    command(
        "chown",
        &[
            &format!("root:{}", c.runtime.nginx_user),
            path(tmp.path())?,
            path(&candidate)?,
        ],
    )?;
    command("chmod", &["0750", path(tmp.path())?])?;
    command("chmod", &["0640", path(&candidate)?])?;
    command(
        "runuser",
        &[
            "-u",
            &c.runtime.xray_user,
            "-g",
            &c.runtime.nginx_user,
            "--",
            &c.runtime.xray_binary,
            "run",
            "-test",
            "-config",
            path(&candidate)?,
        ],
    )?;
    command(
        &c.runtime.nginx_binary,
        &["-t", "-c", path(&release.join("nginx.conf"))?],
    )?;
    let xray_unit = release.join("systemd/xray.service");
    let nginx_unit = release.join("systemd/nginx.service");
    let warp_unit = release.join("systemd/usque-warp.service");
    let mut verify_units = vec!["verify", path(&xray_unit)?, path(&nginx_unit)?];
    if c.warp.enabled {
        verify_units.push(path(&warp_unit)?);
    }
    command("systemd-analyze", &verify_units)?;
    if c.warp.enabled {
        trusted_binary(Path::new(&c.runtime.usque_binary))?;
        store::check_private(Path::new(&c.warp.config_file), false)?;
        ensure!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&c.warp.config_file)?).is_ok(),
            "Invalid existing WARP credentials"
        );
        ensure!(Path::new("/dev/net/tun").exists(), "Missing TUN device");
    }
    Ok((c, s))
}

pub fn check(store: &Store, id: &str, reference: Reference) -> Result<()> {
    linux_root()?;
    store.release(id)?;
    let receipt_path = store.state_dir.join("checks").join(id);
    // A failed recheck invalidates an older receipt.
    if receipt_path.exists() {
        fs::remove_file(&receipt_path)?;
    }
    let (c, _) = preflight(store, id, &reference)?;
    let receipt = Receipt {
        release: id.into(),
        dependencies: dependencies(&c, &reference)?,
        reference,
    };
    store::private_dir(receipt_path.parent().unwrap())?;
    store::atomic_write(&receipt_path, &serde_json::to_vec_pretty(&receipt)?)
}

fn receipt(store: &Store, id: &str) -> Result<Receipt> {
    let p = store.state_dir.join("checks").join(id);
    store::check_private(&p, false)
        .context("Candidate has no valid check receipt; run check first")?;
    let r: Receipt = serde_json::from_slice(&fs::read(p)?)?;
    ensure!(r.release == id, "Receipt release mismatch");
    let (c, _) = store.verify(id)?;
    r.reference.verify()?;
    ensure!(
        r.dependencies == dependencies(&c, &r.reference)?,
        "Runtime binaries, certificates or WARP configuration changed; check again"
    );
    Ok(r)
}

const ENTRIES: [(&str, &str); 5] = [
    ("/etc/xray/config.json", "xray.json"),
    ("/etc/nginx/nginx.conf", "nginx.conf"),
    ("/etc/systemd/system/xray.service", "systemd/xray.service"),
    ("/etc/systemd/system/nginx.service", "systemd/nginx.service"),
    (
        "/etc/systemd/system/usque-warp.service",
        "systemd/usque-warp.service",
    ),
];
const SERVICES: [&str; 3] = ["xray", "nginx", "usque-warp"];

fn checkpoint(_name: &str) -> Result<()> {
    #[cfg(feature = "integration-test")]
    ensure!(
        !std::env::var("NODECTL_FAILPOINT")
            .ok()
            .is_some_and(|value| value.split(',').any(|stage| stage == _name)),
        "Injected integration failure: {_name}"
    );
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Entry {
    Missing,
    Link {
        target: PathBuf,
    },
    File {
        sha256: String,
        backup: PathBuf,
        mode: u32,
        uid: u32,
        gid: u32,
    },
}
#[derive(Clone, Serialize, Deserialize)]
struct Service {
    active: bool,
    enabled: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Transaction {
    candidate: String,
    current: Option<String>,
    previous: Option<String>,
    entries: BTreeMap<String, Entry>,
    services: BTreeMap<String, Service>,
    routes: String,
    routes6: String,
    backup: PathBuf,
}

fn entry(p: &Path, backup: &Path) -> Result<Entry> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Entry::Missing),
        Err(e) => Err(e.into()),
        Ok(m) if m.file_type().is_symlink() => Ok(Entry::Link {
            target: fs::read_link(p)?,
        }),
        Ok(m) => {
            ensure!(
                m.is_file(),
                "Cannot take over non-file entrypoint: {}",
                p.display()
            );
            let (mode, uid, gid) = metadata(&m);
            store::atomic_write(backup, &fs::read(p)?)?;
            Ok(Entry::File {
                sha256: store::file_digest(p)?,
                backup: backup.into(),
                mode,
                uid,
                gid,
            })
        }
    }
}
fn metadata(m: &fs::Metadata) -> (u32, u32, u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (m.mode() & 0o7777, m.uid(), m.gid())
    }
    #[cfg(not(unix))]
    {
        (0, 0, 0)
    }
}
fn fingerprint(e: &Entry) -> String {
    match e {
        Entry::Missing => "missing".into(),
        Entry::Link { target } => format!("link:{}", target.display()),
        Entry::File {
            sha256,
            mode,
            uid,
            gid,
            ..
        } => format!("file:{sha256}:{mode}:{uid}:{gid}"),
    }
}

pub fn takeover(store: &Store, id: &str) -> Result<()> {
    linux_root()?;
    store.verify(id)?;
    let dir = store.state_dir.join("takeover").join(id);
    ensure!(
        !dir.exists(),
        "Takeover record already exists; retain its backups"
    );
    store::private_dir(&dir)?;
    let mut authorization = BTreeMap::new();
    let mut diff = String::from(
        "Explicit takeover: existing entries are backed up, not modified.\nContent withheld to protect unmanaged credentials.\n",
    );
    for (i, (dest, source)) in ENTRIES.iter().enumerate() {
        let e = entry(Path::new(dest), &dir.join(format!("backup-{i}")))?;
        let old = fingerprint(&e);
        let next = store::file_digest(&store.release(id)?.join(source))?;
        diff.push_str(&format!(
            "{dest}\n  before: {old}\n  after: sha256:{next}\n"
        ));
        authorization.insert(dest.to_string(), e);
    }
    store::atomic_write(
        &dir.join("entries.json"),
        &serde_json::to_vec_pretty(&authorization)?,
    )?;
    store::atomic_write(&dir.join("diff.txt"), diff.as_bytes())
}

fn authorized(
    store: &Store,
    id: &str,
    current: Option<&str>,
    entries: &BTreeMap<String, Entry>,
) -> Result<()> {
    let p = store
        .state_dir
        .join("takeover")
        .join(id)
        .join("entries.json");
    let approvals: BTreeMap<String, Entry> = if p.exists() {
        store::check_private(&p, false)?;
        serde_json::from_slice(&fs::read(p)?)?
    } else {
        BTreeMap::new()
    };
    for (dest, source) in ENTRIES {
        let existing = &entries[dest];
        let managed = if let (Some(current), Entry::Link { target }) = (current, existing) {
            *target == store.release(current)?.join(source)
        } else {
            false
        };
        ensure!(
            matches!(existing, Entry::Missing)
                || managed
                || approvals
                    .get(dest)
                    .is_some_and(|e| fingerprint(e) == fingerprint(existing)),
            "Unmanaged entrypoint {dest}; run takeover {id} to explicitly back up and authorize takeover"
        );
    }
    Ok(())
}

fn service_state(name: &str) -> Result<Service> {
    let load = command("systemctl", &["show", name, "-p", "LoadState", "--value"])?;
    let enabled = if load.trim() == "not-found" {
        "disabled".into()
    } else {
        command(
            "systemctl",
            &["show", name, "-p", "UnitFileState", "--value"],
        )?
        .trim()
        .to_owned()
    };
    ensure!(
        ["enabled", "enabled-runtime", "disabled", "static", ""].contains(&enabled.as_str()),
        "Unsupported existing service enable state for {name}; preserve it explicitly before takeover"
    );
    let active = command("systemctl", &["show", name, "-p", "ActiveState", "--value"])?;
    ensure!(
        ["active", "inactive", "failed"].contains(&active.trim()),
        "Service {name} is transitioning; try again when stable"
    );
    Ok(Service {
        active: active.trim() == "active",
        enabled,
    })
}

fn default_routes(ipv6: bool) -> Result<String> {
    let output = command(
        "ip",
        &[
            if ipv6 { "-6" } else { "-4" },
            "-j",
            "route",
            "show",
            "table",
            "all",
            "default",
        ],
    )?;
    let mut routes: Vec<serde_json::Value> = serde_json::from_str(&output)?;
    for route in &mut routes {
        if let Some(object) = route.as_object_mut() {
            for field in ["expires", "used", "age", "lastuse"] {
                object.remove(field);
            }
        }
    }
    routes.sort_by_key(serde_json::Value::to_string);
    Ok(serde_json::to_string(&routes)?)
}

pub fn apply(store: &Store, id: &str) -> Result<()> {
    linux_root()?;
    store.release(id)?;
    let r = receipt(store, id)?;
    let (c, s) = preflight(store, id, &r.reference)?;
    let current = store.pointer("current")?;
    let previous = store.pointer("previous")?;
    if current.as_deref() == Some(id) {
        for (dest, source) in ENTRIES {
            ensure!(
                fs::read_link(dest).ok() == Some(store.release(id)?.join(source)),
                "Applied entrypoint has drifted: {dest}"
            );
        }
        health(&c, &s, &r.reference)?;
        return Ok(());
    }
    let backup = store
        .state_dir
        .join("backups")
        .join(uuid::Uuid::new_v4().to_string());
    store::private_dir(&backup)?;
    let mut entries = BTreeMap::new();
    for (i, (dest, _)) in ENTRIES.iter().enumerate() {
        entries.insert(
            dest.to_string(),
            entry(Path::new(dest), &backup.join(format!("entry-{i}")))?,
        );
    }
    authorized(store, id, current.as_deref(), &entries)?;
    let services = SERVICES
        .into_iter()
        .map(|name| Ok((name.to_owned(), service_state(name)?)))
        .collect::<Result<_>>()?;
    let tx = Transaction {
        candidate: id.into(),
        current,
        previous,
        entries,
        services,
        routes: default_routes(false)?,
        routes6: default_routes(true)?,
        backup,
    };
    let journal = store.state_dir.join("transaction.json");
    // Keep the original host state across later applies and upgrades.
    if tx.current.is_none() {
        store::atomic_write(
            &store.state_dir.join("installation.json"),
            &serde_json::to_vec_pretty(&tx)?,
        )?;
    }
    store::atomic_write(&journal, &serde_json::to_vec_pretty(&tx)?)?;
    checkpoint("journal")?;
    transact(
        || activate(store, id, &c, &s, &r, &tx),
        || restore(store, &tx),
        || {
            store::atomic_write(&store.state_dir.join("recovery-required.txt"),
            format!("Candidate: {id}\nJournal: {}\nBackups: {}\nNo successful marker was committed. Inspect and restore entries/service states before removing transaction.json.\n",journal.display(),tx.backup.display()).as_bytes())
        },
    )?;
    fs::remove_file(&journal)?;
    // Pruning is housekeeping after a committed transaction, not a reason to
    // roll back healthy services or report the deployment itself as failed.
    if store.prune().is_err() {
        eprintln!("nodectl: deployment succeeded; old-release cleanup needs attention");
    }
    Ok(())
}

fn transact<A, R, F>(activate: A, restore: R, on_restore_failure: F) -> Result<()>
where
    A: FnOnce() -> Result<()>,
    R: FnOnce() -> Result<()>,
    F: FnOnce() -> Result<()>,
{
    if let Err(error) = activate() {
        match restore() {
            Ok(()) => {
                bail!("Apply failed: {error:#}; previous entrypoints and service states restored")
            }
            Err(recovery) => {
                let _ = on_restore_failure();
                bail!(
                    "Apply failed: {error:#}; RECOVERY FAILED: {recovery:#}; transaction.json and backups retained"
                )
            }
        }
    }
    Ok(())
}

fn changed(store: &Store, id: &str, old: Option<&str>, name: &str) -> Result<bool> {
    Ok(if let Some(old) = old {
        fs::read(store.release(id)?.join(name))? != fs::read(store.release(old)?.join(name))?
    } else {
        true
    })
}

fn activate(
    store: &Store,
    id: &str,
    c: &Config,
    s: &Secrets,
    r: &Receipt,
    tx: &Transaction,
) -> Result<()> {
    let old = tx.current.as_deref();
    let old_c = old.map(|v| store.verify(v).map(|p| p.0)).transpose()?;
    let mut unit_changed = false;
    for (index, (dest, source)) in ENTRIES.into_iter().enumerate() {
        fs::create_dir_all(Path::new(dest).parent().unwrap())?;
        if source.starts_with("systemd/") && changed(store, id, old, source)? {
            unit_changed = true;
        }
        store::replace_link(&store.release(id)?.join(source), Path::new(dest))?;
        checkpoint(&format!("entry-{index}"))?;
    }
    if unit_changed {
        command("systemctl", &["daemon-reload"])?;
    }
    checkpoint("daemon-reload")?;
    let xray_changed =
        changed(store, id, old, "xray.json")? || changed(store, id, old, "systemd/xray.service")?;
    if xray_changed || !tx.services["xray"].active {
        command("systemctl", &["restart", "xray"])?;
    }
    command("systemctl", &["enable", "xray"])?;
    checkpoint("xray")?;
    let nginx_unit = changed(store, id, old, "systemd/nginx.service")?;
    let listeners_changed = old_c.as_ref().is_none_or(|old| {
        old.ports.http != c.ports.http
            || old.ports.https != c.ports.https
            || old.node.ipv6.is_empty() != c.node.ipv6.is_empty()
            || old.tls.http3 != c.tls.http3
    });
    if nginx_unit || listeners_changed || !tx.services["nginx"].active {
        command("systemctl", &["restart", "nginx"])?;
    } else if changed(store, id, old, "nginx.conf")? {
        command("systemctl", &["reload", "nginx"])?;
    }
    command("systemctl", &["enable", "nginx"])?;
    checkpoint("nginx")?;
    let warp_changed = changed(store, id, old, "systemd/usque-warp.service")?
        || old_c
            .as_ref()
            .is_none_or(|old| old.warp.enabled != c.warp.enabled);
    // The WARP credential digest is recorded separately from immutable sources.
    let old_warp_digest = old
        .and_then(|id| fs::read(store.state_dir.join("success").join(id)).ok())
        .and_then(|bytes| serde_json::from_slice::<Receipt>(&bytes).ok())
        .and_then(|r| r.dependencies.get(&c.warp.config_file).cloned());
    let warp_config_changed =
        c.warp.enabled && old_warp_digest != r.dependencies.get(&c.warp.config_file).cloned();
    let warp_state = &tx.services["usque-warp"];
    if warp_changed
        || warp_config_changed
        || c.warp.enabled != warp_state.active
        || c.warp.enabled != warp_state.enabled.starts_with("enabled")
    {
        if c.warp.enabled {
            command("systemctl", &["enable", "usque-warp"])?;
            command("systemctl", &["restart", "usque-warp"])?;
        } else if warp_state.active || warp_state.enabled.starts_with("enabled") {
            command("systemctl", &["stop", "usque-warp"])?;
            command("systemctl", &["disable", "usque-warp"])?;
            // disable also removes linked units stored outside /etc and /usr.
            // Retain our managed entrypoint while leaving the service disabled.
            store::replace_link(
                &store.release(id)?.join("systemd/usque-warp.service"),
                Path::new("/etc/systemd/system/usque-warp.service"),
            )?;
            command("systemctl", &["daemon-reload"])?;
        }
    }
    checkpoint("services")?;
    health(c, s, &r.reference)?;
    checkpoint("health")?;
    ensure!(
        tx.routes == default_routes(false)? && tx.routes6 == default_routes(true)?,
        "Default routes changed during deployment"
    );
    store.set_pointer("previous", tx.current.as_deref())?;
    store.set_pointer("current", Some(id))?;
    store::private_dir(&store.state_dir.join("success"))?;
    store::atomic_write(
        &store.state_dir.join("success").join(id),
        &serde_json::to_vec_pretty(r)?,
    )?;
    Ok(())
}

fn restore_entry(dest: &Path, e: &Entry) -> Result<()> {
    match e {
        Entry::Missing => {
            if fs::symlink_metadata(dest).is_ok() {
                fs::remove_file(dest)?;
            }
        }
        Entry::Link { target } => store::replace_link(target, dest)?,
        Entry::File {
            backup,
            sha256,
            mode,
            uid,
            gid,
        } => {
            ensure!(
                store::file_digest(backup)? == *sha256,
                "Backup integrity failure"
            );
            store::atomic_write(dest, &fs::read(backup)?)?;
            command("chown", &[&format!("{uid}:{gid}"), path(dest)?])?;
            command("chmod", &[&format!("{mode:o}"), path(dest)?])?;
        }
    }
    Ok(())
}

fn restore(store: &Store, tx: &Transaction) -> Result<()> {
    let mut errors = vec![];
    let mut attempt = |label: &str, result: Result<()>| {
        if result.is_err() {
            errors.push(label.to_owned());
        }
    };
    // Restore all components even when one recovery action fails.
    for name in SERVICES {
        let loaded = command("systemctl", &["show", name, "-p", "LoadState", "--value"]);
        match loaded {
            Ok(state) if state.trim() == "not-found" => {}
            Ok(_) => {
                attempt(
                    &format!("stop {name}"),
                    command("systemctl", &["stop", name]).map(|_| ()),
                );
                attempt(
                    &format!("disable {name}"),
                    command("systemctl", &["disable", name]).map(|_| ()),
                );
            }
            Err(e) => attempt(&format!("inspect {name}"), Err(e)),
        }
    }
    for (index, (dest, e)) in tx.entries.iter().enumerate() {
        attempt(
            dest,
            checkpoint(&format!("restore-entry-{index}"))
                .and_then(|_| restore_entry(Path::new(dest), e)),
        );
    }
    attempt(
        "daemon-reload",
        command("systemctl", &["daemon-reload"]).map(|_| ()),
    );
    for name in SERVICES {
        let state = &tx.services[name];
        if state.enabled == "enabled" {
            attempt(
                &format!("enable {name}"),
                command("systemctl", &["enable", name]).map(|_| ()),
            );
        }
        if state.enabled == "enabled-runtime" {
            attempt(
                &format!("enable runtime {name}"),
                command("systemctl", &["enable", "--runtime", name]).map(|_| ()),
            );
        }
        if state.active {
            attempt(
                &format!("restart {name}"),
                command("systemctl", &["restart", name]).map(|_| ()),
            );
        }
    }
    attempt(
        "restore current pointer",
        store.set_pointer("current", tx.current.as_deref()),
    );
    attempt(
        "restore previous pointer",
        store.set_pointer("previous", tx.previous.as_deref()),
    );
    if let Some(id) = &tx.current {
        let result = (|| {
            let r = receipt(store, id)?;
            let (c, s) = preflight(store, id, &r.reference)?;
            health(&c, &s, &r.reference)
        })();
        attempt("previous release checks and live health", result);
    } else {
        for name in SERVICES {
            let was_active = tx.services[name].active;
            attempt(
                &format!("restored service state {name}"),
                (|| {
                    let active =
                        command("systemctl", &["show", name, "-p", "ActiveState", "--value"])?;
                    ensure!(
                        (active.trim() == "active") == was_active,
                        "Service state mismatch"
                    );
                    Ok(())
                })(),
            );
        }
    }
    attempt(
        "default route preservation",
        (move || {
            ensure!(
                tx.routes == default_routes(false)? && tx.routes6 == default_routes(true)?,
                "Routes differ"
            );
            Ok(())
        })(),
    );
    ensure!(
        errors.is_empty(),
        "Recovery failures: {}",
        errors.join(", ")
    );
    let success = store.state_dir.join("success").join(&tx.candidate);
    if tx.current.as_deref() != Some(tx.candidate.as_str()) && success.exists() {
        fs::remove_file(success)?;
    }
    fs::remove_file(store.state_dir.join("transaction.json"))?;
    Ok(())
}

fn retry<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        match f() {
            Ok(value) => return Ok(value),
            Err(e) if start.elapsed() >= Duration::from_secs(12) => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn health(c: &Config, s: &Secrets, reference: &Reference) -> Result<()> {
    for name in ["xray", "nginx"] {
        command("systemctl", &["is-active", "--quiet", name])?;
    }
    if c.warp.enabled {
        command("systemctl", &["is-active", "--quiet", "usque-warp"])?;
    }
    for port in [c.ports.http, c.ports.https, c.ports.reality] {
        retry(|| {
            TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse()?,
                Duration::from_secs(1),
            )
            .map_err(Into::into)
        })?;
    }
    if !c.node.ipv6.is_empty() {
        for port in [c.ports.http, c.ports.https, c.ports.reality] {
            retry(|| {
                TcpStream::connect_timeout(
                    &format!("[::1]:{port}").parse()?,
                    Duration::from_secs(1),
                )
                .map_err(Into::into)
            })?;
        }
    }
    for socket in [&c.transport.xhttp.socket, &c.transport.ws.socket] {
        retry(|| {
            let m = fs::symlink_metadata(socket)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::{FileTypeExt, MetadataExt};
                ensure!(
                    m.file_type().is_socket() && m.mode() & 0o777 == 0o660,
                    "UDS missing or permissions differ from 0660"
                );
                let uid = command("id", &["-u", &c.runtime.xray_user])?
                    .trim()
                    .parse::<u32>()?;
                let gid = command("getent", &["group", &c.runtime.nginx_user])?
                    .split(':')
                    .nth(2)
                    .context("Missing Nginx group")?
                    .parse::<u32>()?;
                ensure!(m.uid() == uid && m.gid() == gid, "UDS ownership mismatch");
                std::os::unix::net::UnixStream::connect(socket)?;
            }
            access(&c.runtime.nginx_user, None, "-w", socket)?;
            Ok(())
        })?;
    }
    if c.tls.http3 {
        let udp = command(
            "ss",
            &["-H", "-lun", "sport", "=", &format!(":{}", c.ports.https)],
        )?;
        ensure!(!udp.trim().is_empty(), "QUIC UDP listener is missing");
    }
    reference.verify()?;
    for kind in ["xhttp", "ws", "reality"] {
        handshake(c, s, reference, kind)
            .with_context(|| format!("{kind} live handshake failed"))?;
    }
    for name in ["xray", "nginx"]
        .into_iter()
        .chain(c.warp.enabled.then_some("usque-warp"))
    {
        command("systemctl", &["is-active", "--quiet", name])?;
    }
    Ok(())
}

fn handshake(c: &Config, s: &Secrets, reference: &Reference, kind: &str) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let mut outbound = render::outbound(c, s, kind, "chrome")?;
    // Local loopback is the server under test; retain the real SNI and Host.
    outbound["settings"]["vnext"][0]["address"] = json!("127.0.0.1");
    let profile = json!({"log":{"loglevel":"none"},"inbounds":[{"listen":"127.0.0.1","port":port,
        "protocol":"socks","settings":{"auth":"noauth","udp":false}}],"outbounds":[outbound]});
    let config = tmp.path().join("client.json");
    store::atomic_write(&config, &serde_json::to_vec(&profile)?)?;
    command(
        path(&reference.binary)?,
        &["run", "-test", "-config", path(&config)?],
    )?;
    drop(listener);
    let child = Command::new(&reference.binary)
        .args(["run", "-config", path(&config)?])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut guard = Guard(child);
    retry(|| {
        ensure!(guard.0.try_wait()?.is_none(), "Reference client exited");
        command(
            "curl",
            &[
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "8",
                "--noproxy",
                "",
                "--socks5-hostname",
                &format!("127.0.0.1:{port}"),
                "--output",
                "/dev/null",
                &reference.probe_url,
            ],
        )
        .map(|_| ())
    })
}

pub fn renew(store: &Store) -> Result<()> {
    linux_root()?;
    let id = store.pointer("current")?.context("No applied release")?;
    let (c, _) = store.verify(&id)?;
    certificates(&c)?;
    command(
        &c.runtime.nginx_binary,
        &["-t", "-c", path(&store.release(&id)?.join("nginx.conf"))?],
    )?;
    command("systemctl", &["reload", "nginx"])?;
    // Refresh certificates in the receipt so a later rollback can still check
    // binaries against their original pins after legitimate renewal.
    for release in [Some(id), store.pointer("previous")?].into_iter().flatten() {
        let p = store.state_dir.join("checks").join(&release);
        if !p.exists() {
            continue;
        }
        let mut r: Receipt = serde_json::from_slice(&fs::read(&p)?)?;
        let (config, _) = store.verify(&release)?;
        certificates(&config)?;
        let new = dependencies(&config, &r.reference)?;
        for (file, hash) in new {
            if file.ends_with("/fullchain.pem") || file.ends_with("/privkey.pem") {
                r.dependencies.insert(file, hash);
            }
        }
        store::atomic_write(&p, &serde_json::to_vec_pretty(&r)?)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct UninstallJournal {
    rollback: Transaction,
}

fn validate_transaction(store: &Store, tx: &Transaction) -> Result<()> {
    store.release(&tx.candidate)?;
    ensure!(
        tx.backup.starts_with(store.state_dir.join("backups")),
        "Backup escapes state directory"
    );
    store::check_private(&tx.backup, true)?;
    ensure!(
        tx.entries.len() == ENTRIES.len()
            && ENTRIES
                .iter()
                .all(|(dest, _)| tx.entries.contains_key(*dest)),
        "Unexpected transaction entrypoints"
    );
    ensure!(
        tx.services.len() == SERVICES.len()
            && SERVICES.iter().all(|s| tx.services.contains_key(*s)),
        "Unexpected transaction services"
    );
    for entry in tx.entries.values() {
        if let Entry::File { backup, sha256, .. } = entry {
            ensure!(
                backup.parent() == Some(tx.backup.as_path()),
                "Backup file outside transaction directory"
            );
            store::check_private(backup, false)?;
            ensure!(
                store::file_digest(backup)? == *sha256,
                "Backup integrity failure"
            );
        }
    }
    Ok(())
}

pub fn uninstall(store: &Store, execute: bool) -> Result<()> {
    let current = store
        .pointer("current")?
        .context("No installed successful release")?;
    let baseline_path = store.state_dir.join("installation.json");
    store::check_private(&baseline_path,false).context("No original installation baseline. Legacy installs must recover their original entrypoints and service states explicitly; refusing destructive guesses")?;
    let mut baseline: Transaction = serde_json::from_slice(&fs::read(baseline_path)?)?;
    validate_transaction(store, &baseline)?;
    ensure!(
        baseline.current.is_none() && baseline.previous.is_none(),
        "Invalid installation baseline"
    );
    for (dest, source) in ENTRIES {
        ensure!(
            fs::read_link(dest).ok() == Some(store.release(&current)?.join(source)),
            "Managed entrypoint drifted: {dest}; resolve before uninstall"
        );
        println!("Restore {dest} to {}", fingerprint(&baseline.entries[dest]));
    }
    for (name, state) in &baseline.services {
        println!(
            "Restore service {name}: active={}, enabled={}",
            state.active, state.enabled
        );
    }
    println!(
        "Retain certificates, WARP credentials, configuration, snapshots, backup records and nodectl executable."
    );
    if !execute {
        println!("Preview only; rerun uninstall --execute.");
        return Ok(());
    }
    linux_root()?;
    let backup = store
        .state_dir
        .join("backups")
        .join(uuid::Uuid::new_v4().to_string());
    store::private_dir(&backup)?;
    let mut entries = BTreeMap::new();
    for (i, (dest, _)) in ENTRIES.iter().enumerate() {
        entries.insert(
            dest.to_string(),
            entry(Path::new(dest), &backup.join(format!("entry-{i}")))?,
        );
    }
    let rollback = Transaction {
        candidate: current.clone(),
        current: Some(current),
        previous: store.pointer("previous")?,
        entries,
        services: SERVICES
            .into_iter()
            .map(|name| Ok((name.to_owned(), service_state(name)?)))
            .collect::<Result<_>>()?,
        routes: default_routes(false)?,
        routes6: default_routes(true)?,
        backup,
    };
    // Network changes made since installation are not owned by uninstall.
    baseline.routes = rollback.routes.clone();
    baseline.routes6 = rollback.routes6.clone();
    let pending = store.state_dir.join("uninstall.json");
    store::atomic_write(
        &pending,
        &serde_json::to_vec_pretty(&UninstallJournal {
            rollback: rollback.clone(),
        })?,
    )?;
    checkpoint("uninstall-journal")?;
    let journal = store.state_dir.join("transaction.json");
    store::atomic_write(&journal, &serde_json::to_vec_pretty(&baseline)?)?;
    if restore(store, &baseline).is_err() {
        store::atomic_write(&journal, &serde_json::to_vec_pretty(&rollback)?)?;
        if restore(store, &rollback).is_err() {
            bail!(
                "Uninstall and recovery failed; journals retained. Run recover before further mutations."
            );
        }
        fs::remove_file(pending)?;
        bail!("Uninstall failed; installed entrypoints and services restored");
    }
    fs::remove_file(pending)?;
    println!("Original entrypoints and service states restored and verified.");
    Ok(())
}

pub fn recover(store: &Store) -> Result<()> {
    linux_root()?;
    let uninstall = store.state_dir.join("uninstall.json");
    if uninstall.exists() {
        store::check_private(&uninstall, false)?;
        let pending: UninstallJournal = serde_json::from_slice(&fs::read(&uninstall)?)?;
        validate_transaction(store, &pending.rollback)?;
        store::atomic_write(
            &store.state_dir.join("transaction.json"),
            &serde_json::to_vec_pretty(&pending.rollback)?,
        )?;
        restore(store, &pending.rollback)?;
        fs::remove_file(uninstall)?;
        return Ok(());
    }
    let journal = store.state_dir.join("transaction.json");
    store::check_private(&journal, false)?;
    let tx: Transaction = serde_json::from_slice(&fs::read(journal)?)?;
    ensure!(
        tx.backup.starts_with(store.state_dir.join("backups")),
        "Backup escapes state directory"
    );
    ensure!(
        tx.entries.len() == ENTRIES.len()
            && ENTRIES
                .iter()
                .all(|(dest, _)| tx.entries.contains_key(*dest)),
        "Unexpected journal entrypoints"
    );
    ensure!(
        tx.services.len() == SERVICES.len()
            && SERVICES.iter().all(|s| tx.services.contains_key(*s)),
        "Unexpected journal services"
    );
    restore(store, &tx)?;
    let warning = store.state_dir.join("recovery-required.txt");
    if warning.exists() {
        fs::remove_file(warning)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn failed_apply_restores_and_reports_recovery_failure() {
        let restored = Cell::new(false);
        let result = transact(
            || bail!("reload failed"),
            || {
                restored.set(true);
                Ok(())
            },
            || Ok(()),
        );
        assert!(restored.get());
        assert!(result.unwrap_err().to_string().contains("restored"));
        let retained = Cell::new(false);
        let result = transact(
            || bail!("reload failed"),
            || bail!("restart failed"),
            || {
                retained.set(true);
                Ok(())
            },
        );
        assert!(retained.get());
        assert!(result.unwrap_err().to_string().contains("RECOVERY FAILED"));
    }
    #[test]
    fn successful_apply_does_not_run_recovery() {
        transact(
            || Ok(()),
            || panic!("unexpected restore"),
            || panic!("unexpected journal"),
        )
        .unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn failed_switch_restores_actual_entrypoint_content_and_mode() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("config");
        let candidate = tmp.path().join("candidate");
        fs::write(&dest, b"old protected config").unwrap();
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o640)).unwrap();
        fs::write(&candidate, b"new config").unwrap();
        let backup = entry(&dest, &tmp.path().join("backup")).unwrap();
        let result = transact(
            || {
                store::replace_link(&candidate, &dest)?;
                ensure!(
                    fs::read_link(&dest)? == candidate,
                    "Candidate not activated"
                );
                bail!("simulated failure after switching entrypoint")
            },
            || restore_entry(&dest, &backup),
            || Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old protected config");
        assert!(
            !fs::symlink_metadata(&dest)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn damaged_backup_is_rejected_and_recovery_record_is_retained() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("config");
        fs::write(&dest, b"old").unwrap();
        let backup_file = tmp.path().join("backup");
        let backup = entry(&dest, &backup_file).unwrap();
        fs::write(&backup_file, b"corrupted").unwrap();
        let journal = tmp.path().join("recovery-required");
        let result = transact(
            || bail!("switch failed"),
            || restore_entry(&dest, &backup),
            || store::atomic_write(&journal, b"manual recovery required"),
        );
        assert!(result.unwrap_err().to_string().contains("RECOVERY FAILED"));
        assert!(journal.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
    }
}
