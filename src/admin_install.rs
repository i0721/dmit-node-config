//! Dedicated HTTPS listener; never edits node templates, current or node inputs.
use crate::{
    admin_auth, config, host,
    store::{self, Store},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
const ROOT: &str = "/usr/local/share/nodectl-admin";
const CONFIG: &str = "/etc/nodectl-admin";
const MARKER: &str = "# Managed by nodectl admin";
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetManifest {
    version: String,
    files: BTreeMap<String, String>,
}
fn archive_member(archive: &Path, name: &str, limit: u64) -> Result<Vec<u8>> {
    let mut child = Command::new("tar")
        .args(["-xOzf"])
        .arg(archive)
        .args(["--", name])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut bytes = Vec::new();
    let result = child
        .stdout
        .take()
        .context("Archive stream")?
        .take(limit + 1)
        .read_to_end(&mut bytes);
    if result.is_err() || bytes.len() as u64 > limit {
        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!("Archive member exceeds its limit");
    }
    ensure!(child.wait()?.success(), "Missing archive member");
    Ok(bytes)
}
fn unpack_assets(archive: &Path, destination: &Path) -> Result<()> {
    ensure!(
        archive.is_absolute() && fs::symlink_metadata(archive)?.is_file(),
        "Regular absolute archive required"
    );
    let manifest: AssetManifest = serde_json::from_slice(&archive_member(
        archive,
        "webui-manifest.json",
        2 * 1024 * 1024,
    )?)?;
    ensure!(
        manifest.version == env!("CARGO_PKG_VERSION"),
        "WebUI/API version mismatch"
    );
    ensure!(
        !manifest.files.is_empty() && manifest.files.len() <= 10000,
        "Static asset limit"
    );
    let mut total = 0;
    for (name, hash) in manifest.files {
        ensure!(
            !name.split('/').any(|p| p.is_empty() || p.starts_with('.'))
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_!.-".contains(&b)),
            "Unsafe static archive path"
        );
        ensure!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid asset digest"
        );
        let bytes = archive_member(archive, &format!("webui/{name}"), 100 * 1024 * 1024 - total)?;
        total += bytes.len() as u64;
        ensure!(
            store::digest(&bytes) == hash,
            "Static archive digest mismatch"
        );
        let target = destination.join(name);
        fs::create_dir_all(target.parent().context("Static parent")?)?;
        store::atomic_write(&target, &bytes)?;
    }
    assets_files(destination)?;
    Ok(())
}
pub fn from_archive(s: &Store, archive: &Path, port: u16, execute: bool) -> Result<()> {
    let temp = tempfile::tempdir()?;
    unpack_assets(archive, temp.path())?;
    run(s, temp.path(), port, execute)
}
pub(crate) fn renew_if_installed(c: &config::Config) -> Result<()> {
    let path = Path::new("/etc/nodectl-admin/nginx.conf");
    if !path.exists() {
        return Ok(());
    }
    store::check_private(Path::new(CONFIG), true)?;
    store::check_private(path, false)?;
    ensure!(
        fs::read_to_string(path)?.starts_with(MARKER),
        "Unmanaged admin proxy configuration"
    );
    if host::command(
        "systemctl",
        &["is-active", "--quiet", "nodectl-admin-nginx"],
    )
    .is_ok()
    {
        host::command(
            &c.runtime.nginx_binary,
            &["-t", "-c", "/etc/nodectl-admin/nginx.conf"],
        )?;
        host::command("systemctl", &["reload", "nodectl-admin-nginx"])?;
    }
    Ok(())
}
pub fn run(s: &Store, assets: &Path, port: u16, execute: bool) -> Result<()> {
    let (c, _) = s.inputs()?;
    ensure!(
        ![c.ports.http, c.ports.https, c.ports.reality, 9080].contains(&port),
        "Admin HTTPS port collides with node or API listeners"
    );
    for path in [
        s.config_dir.as_path(),
        s.state_dir.as_path(),
        std::env::current_exe()?.as_path(),
    ] {
        ensure!(
            config::abs_path(path.to_str().context("path")?),
            "Unsafe installation path"
        );
    }
    let files = assets_files(assets)?;
    ensure!(
        files.contains_key("index.html") && files.keys().any(|p| p.starts_with("_next/static/")),
        "Build the static frontend first"
    );
    let digest = store::digest(&serde_json::to_vec(&files)?);
    let origin = format!("https://{}:{port}", c.node.domain);
    let binary = std::env::current_exe()?;
    let api = format!(
        "{MARKER}\n[Unit]\nDescription=DMIT single administrator API\nAfter=network-online.target\n[Service]\nType=simple\nExecStart={} --config-dir {} --state-dir {} admin serve --origin {}\nRestart=on-failure\nRestartSec=3\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=false\n[Install]\nWantedBy=multi-user.target\n",
        binary.display(),
        s.config_dir.display(),
        s.state_dir.display(),
        origin
    );
    let mut hashes = std::collections::BTreeSet::new();
    let scripts = regex::Regex::new(r"(?s)<script([^>]*)>(.*?)</script>")?;
    for (name, hash) in files.iter().filter(|(name, _)| name.ends_with(".html")) {
        let bytes = fs::read(assets.join(name))?;
        ensure!(store::digest(&bytes) == *hash, "Static input changed");
        let html = std::str::from_utf8(&bytes)?;
        for script in scripts.captures_iter(html) {
            if !script[1].contains("src=") && !script[2].is_empty() {
                hashes.insert(format!(
                    "'sha256-{}'",
                    STANDARD.encode(Sha256::digest(script[2].as_bytes()))
                ));
            }
        }
    }
    ensure!(hashes.len() < 200, "Static CSP hash limit");
    let csp = format!(
        "default-src 'self'; script-src 'self' {}; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'",
        hashes.into_iter().collect::<Vec<_>>().join(" ")
    );
    let nginx = nginx(&c, port, &csp);
    let proxy = format!(
        "{MARKER}\n[Unit]\nDescription=DMIT admin dedicated Nginx HTTPS\nAfter=network-online.target nodectl-admin.service\nWants=nodectl-admin.service\n[Service]\nType=forking\nPIDFile=/run/nodectl-admin-nginx.pid\nExecStartPre={} -t -c {CONFIG}/nginx.conf\nExecStart={} -c {CONFIG}/nginx.conf\nExecReload={} -s reload -c {CONFIG}/nginx.conf\nExecStop={} -s quit -c {CONFIG}/nginx.conf\nRestart=on-failure\n[Install]\nWantedBy=multi-user.target\n",
        c.runtime.nginx_binary,
        c.runtime.nginx_binary,
        c.runtime.nginx_binary,
        c.runtime.nginx_binary
    );
    let owned = [
        ("/etc/systemd/system/nodectl-admin.service", api),
        ("/etc/systemd/system/nodectl-admin-nginx.service", proxy),
        ("/etc/nodectl-admin/nginx.conf", nginx),
    ];
    let mut before = BTreeMap::new();
    for (path, _) in &owned {
        if Path::new(path).exists() {
            let m = fs::symlink_metadata(path)?;
            ensure!(
                m.is_file() && !m.file_type().is_symlink(),
                "Refusing non-regular admin entry"
            );
            let text = fs::read_to_string(path)?;
            ensure!(text.starts_with(MARKER), "Refusing unmanaged admin entry");
            before.insert(*path, Some(text));
        } else {
            before.insert(*path, None);
        }
    }
    println!(
        "Admin HTTPS: {origin}/admin/\nStatic release: {digest}\nNode configuration, original Nginx instance and port 443 remain untouched."
    );
    if !execute {
        println!("Preview only; add --execute to install these dedicated services.");
        return Ok(());
    }
    host::linux_root()?;
    let _guard = s.lock()?;
    host::certificates(&c)?;
    host::command(&c.runtime.nginx_binary, &["-v"])?;
    let previous = fs::read_link(format!("{ROOT}/current")).ok();
    if Path::new(&format!("{ROOT}/current")).exists() {
        ensure!(previous.is_some(), "Refusing unmanaged static root");
    }
    let destination = PathBuf::from(ROOT)
        .join("releases")
        .join(&digest)
        .join("admin");
    public_dir(&destination)?;
    for (name, hash) in &files {
        let bytes = fs::read(assets.join(name))?;
        ensure!(
            store::digest(&bytes) == *hash,
            "Static input changed during install"
        );
        let target = destination.join(name);
        public_dir(target.parent().unwrap())?;
        store::atomic_write(&target, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o644))?;
        }
    }
    store::private_dir(Path::new(CONFIG))?;
    let old_states = [
        ("nodectl-admin", host::service_state("nodectl-admin")?),
        (
            "nodectl-admin-nginx",
            host::service_state("nodectl-admin-nginx")?,
        ),
    ];
    // Credential must exist before systemd starts the API. It remains independent
    // of service installation rollback and is never included in static files.
    if let Some(token) = admin_auth::reset(s, false)? {
        println!("Administrator token (save securely; shown once):\n{token}");
    }
    let result = (|| -> Result<()> {
        store::replace_link(
            &PathBuf::from("releases").join(&digest),
            &PathBuf::from(ROOT).join("current"),
        )?;
        for (path, text) in &owned {
            store::atomic_write(Path::new(path), text.as_bytes())?;
        }
        host::command(
            &c.runtime.nginx_binary,
            &["-t", "-c", "/etc/nodectl-admin/nginx.conf"],
        )?;
        host::command("systemctl", &["daemon-reload"])?;
        host::command(
            "systemctl",
            &["enable", "nodectl-admin", "nodectl-admin-nginx"],
        )?;
        host::command(
            "systemctl",
            &["restart", "nodectl-admin", "nodectl-admin-nginx"],
        )?;
        host::command(
            "systemctl",
            &[
                "is-active",
                "--quiet",
                "nodectl-admin",
                "nodectl-admin-nginx",
            ],
        )?;
        Ok(())
    })();
    if result.is_err() {
        host::command(
            "systemctl",
            &["stop", "nodectl-admin-nginx", "nodectl-admin"],
        )?;
        host::command(
            "systemctl",
            &["disable", "nodectl-admin-nginx", "nodectl-admin"],
        )?;
        for (path, text) in before {
            if let Some(text) = text {
                store::atomic_write(Path::new(path), text.as_bytes())?;
            } else if Path::new(path).exists() {
                fs::remove_file(path)?;
            }
        }
        let link = PathBuf::from(ROOT).join("current");
        if let Some(previous) = previous {
            store::replace_link(&previous, &link)?;
        } else if fs::symlink_metadata(&link).is_ok() {
            fs::remove_file(link)?;
        }
        host::command("systemctl", &["daemon-reload"])?;
        for (name, state) in old_states {
            if state.enabled == "enabled" {
                host::command("systemctl", &["enable", name])?;
            } else if state.enabled == "enabled-runtime" {
                host::command("systemctl", &["enable", "--runtime", name])?;
            }
            if state.active {
                host::command("systemctl", &["restart", name])?;
            }
        }
        result?;
    }
    Ok(())
}
fn public_dir(p: &Path) -> Result<()> {
    let root = Path::new(ROOT);
    ensure!(p.starts_with(root), "Static root escape");
    if p != root {
        public_dir(p.parent().context("parent")?)?;
    }
    if p.exists() {
        let m = fs::symlink_metadata(p)?;
        ensure!(
            m.is_dir() && !m.file_type().is_symlink(),
            "Static directory is not trusted"
        );
    } else {
        fs::create_dir(p)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}
fn assets_files(root: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(
        root: &Path,
        p: &Path,
        files: &mut BTreeMap<String, String>,
        size: &mut u64,
    ) -> Result<()> {
        let m = fs::symlink_metadata(p)?;
        ensure!(
            !m.file_type().is_symlink(),
            "Static assets cannot contain symlinks"
        );
        if m.is_dir() {
            for entry in fs::read_dir(p)? {
                walk(root, &entry?.path(), files, size)?;
            }
        } else {
            ensure!(m.is_file(), "Static assets require ordinary files");
            let name = p
                .strip_prefix(root)?
                .to_str()
                .context("path")?
                .replace('\\', "/");
            ensure!(
                !name.split('/').any(|v| v.starts_with('.')),
                "Static assets cannot include hidden/private files"
            );
            ensure!(
                [
                    "html", "js", "css", "json", "txt", "ico", "png", "jpg", "jpeg", "webp", "svg",
                    "woff", "woff2"
                ]
                .contains(&p.extension().and_then(|s| s.to_str()).unwrap_or("")),
                "Unsupported static asset"
            );
            *size += m.len();
            ensure!(
                *size <= 100 * 1024 * 1024 && files.len() < 10000,
                "Static asset limit"
            );
            files.insert(name, store::file_digest(p)?);
        }
        Ok(())
    }
    ensure!(root.is_absolute(), "Static source must be absolute");
    let mut files = BTreeMap::new();
    walk(root, root, &mut files, &mut 0)?;
    Ok(files)
}
pub(crate) fn nginx(c: &config::Config, port: u16, csp: &str) -> String {
    format!(
        r#"{MARKER}
user {user};
worker_processes auto;
pid /run/nodectl-admin-nginx.pid;
error_log /var/log/nginx/nodectl-admin-error.log warn;
events {{ worker_connections 256; }}
http {{
    types {{ text/html html; text/css css; application/javascript js; application/json json; image/svg+xml svg; image/png png; image/x-icon ico; font/woff2 woff2; text/plain txt; }}
    default_type application/octet-stream;
    access_log off;
    server_tokens off;
    limit_req_zone $binary_remote_addr zone=admin_requests:1m rate=2r/s;
    limit_req_zone $binary_remote_addr zone=admin_login:1m rate=10r/m;
    server {{
        listen {port} ssl;
        listen [::]:{port} ssl;
        server_name {domain};
        ssl_protocols TLSv1.3;
        http2 on;
        ssl_certificate {cert}/fullchain.pem;
        ssl_certificate_key {cert}/privkey.pem;
        root {ROOT}/current;
        client_max_body_size 128k;
        add_header X-Content-Type-Options nosniff always;
        add_header Referrer-Policy no-referrer always;
        add_header X-Frame-Options DENY always;
        add_header Cache-Control "private, no-store" always;
        add_header Content-Security-Policy "{csp}" always;
        location = / {{ return 302 /admin/; }}
        location = /admin {{ return 308 /admin/; }}
        location = /admin/api/login {{
            error_log /dev/null;
            limit_req zone=admin_login burst=3 nodelay;
            limit_req_status 429;
            proxy_pass http://127.0.0.1:9080;
            proxy_set_header Host $host;
            proxy_read_timeout 15s;
        }}
        location ^~ /admin/api/ {{
            error_log /dev/null;
            limit_req zone=admin_requests burst=20 nodelay;
            limit_req_status 429;
            proxy_pass http://127.0.0.1:9080;
            proxy_set_header Host $host;
            proxy_read_timeout 180s;
            proxy_cache off;
            proxy_buffering off;
        }}
        location /admin/ {{ try_files $uri $uri/index.html =404; }}
        location / {{ return 404; }}
    }}
}}
"#,
        user = c.runtime.nginx_user,
        domain = c.node.domain,
        cert = c.tls.domain_cert_dir
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn archive_checks_version_digest_paths_and_preserves_local_files() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("source");
        let dest = t.path().join("destination");
        fs::create_dir_all(src.join("webui/_next/static")).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("webui/index.html"), "<html></html>").unwrap();
        fs::write(src.join("webui/_next/static/a.js"), "void 0").unwrap();
        let files = assets_files(&src.join("webui")).unwrap();
        let archive = t.path().join("assets.tar.gz");
        let pack = |version: &str, entries: &BTreeMap<String, String>| {
            fs::write(
                src.join("webui-manifest.json"),
                serde_json::to_vec(&serde_json::json!({"version":version,"files":entries}))
                    .unwrap(),
            )
            .unwrap();
            assert!(
                Command::new("tar")
                    .args(["-czf"])
                    .arg(&archive)
                    .arg("-C")
                    .arg(&src)
                    .args(["webui-manifest.json", "webui"])
                    .status()
                    .unwrap()
                    .success()
            );
        };
        pack(env!("CARGO_PKG_VERSION"), &files);
        unpack_assets(&archive, &dest).unwrap();
        assert_eq!(assets_files(&dest).unwrap(), files);
        pack("0.5.1", &files);
        assert!(unpack_assets(&archive, &dest).is_err());
        let mut unsafe_files = files.clone();
        unsafe_files.insert("../escape.html".into(), store::digest(b"bad"));
        pack(env!("CARGO_PKG_VERSION"), &unsafe_files);
        assert!(unpack_assets(&archive, &dest).is_err());
        assert!(!t.path().join("escape.html").exists());
        pack(env!("CARGO_PKG_VERSION"), &files);
        fs::write(src.join("webui/index.html"), "changed").unwrap();
        pack(env!("CARGO_PKG_VERSION"), &files);
        assert!(unpack_assets(&archive, &dest).is_err());
    }
    #[test]
    fn proxy_is_separate_and_static_source_rejects_private_files() {
        let c: config::Config = toml::from_str(config::EXAMPLE).unwrap();
        let text = nginx(&c, 9443, "default-src 'self'");
        assert!(text.contains("listen 9443 ssl"));
        assert!(!text.contains("listen 443 "));
        assert!(!text.contains("proxy_set_header Origin"));
        assert!(text.contains("access_log off"));
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("index.html"), "hi").unwrap();
        assert!(assets_files(t.path()).is_ok());
        fs::write(t.path().join(".env"), "secret").unwrap();
        assert!(assets_files(t.path()).is_err());
    }
}
