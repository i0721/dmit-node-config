//! Public website assets are separate from private snapshots and file storage.
use crate::{
    config::Config,
    render,
    store::{self, Store},
};
use anyhow::{Context, Result, ensure};
use rand::RngCore;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const PUBLIC_ROOT: &str = "/var/www/nodectl-services";

pub const INITIAL_PASSWORD: &str = "/etc/nodectl-services/openlist-initial.env";
pub const MAINTENANCE_FILE: &str = "/var/lib/nodectl-web-guard/active";

pub(crate) fn verify_binaries() -> Result<()> {
    for (component, binary) in [
        ("librespeed", "/usr/local/bin/speedtest-backend"),
        ("openlist", "/usr/local/bin/openlist"),
    ] {
        ensure!(
            store::file_digest(Path::new(binary))?
                == crate::artifacts::component_digest(component)?,
            "Installed website service differs from versions.lock.toml: {component}"
        );
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageMetadata {
    uid: u32,
    gid: u32,
    mode: u32,
}

pub(crate) fn storage_metadata(c: &Config) -> Result<Option<StorageMetadata>> {
    if !c.serves_web() {
        return Ok(None);
    }
    let path = Path::new(&c.web_services.as_ref().unwrap().openlist_files_dir);
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Storage root must be a real directory"
    );
    trusted_parents(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(StorageMetadata {
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.mode() & 0o7777,
        }))
    }
    #[cfg(not(unix))]
    anyhow::bail!("Storage metadata requires Linux")
}

pub(crate) fn restore_storage_metadata(
    c: &Config,
    original: Option<&StorageMetadata>,
) -> Result<()> {
    let Some(original) = original else {
        return Ok(());
    };
    ensure!(
        c.serves_web() && original.mode <= 0o7777,
        "Invalid storage recovery metadata"
    );
    let path = Path::new(&c.web_services.as_ref().unwrap().openlist_files_dir);
    ensure!(
        storage_metadata(c)?.is_some(),
        "Original storage directory is missing"
    );
    // Restore only the original root metadata. Never copy, remove or chown
    // uploaded payloads as part of a deployment rollback.
    crate::host::command(
        "chown",
        &[
            &format!("{}:{}", original.uid, original.gid),
            path.to_str().unwrap(),
        ],
    )?;
    crate::host::command(
        "chmod",
        &[&format!("{:o}", original.mode), path.to_str().unwrap()],
    )?;
    Ok(())
}

#[derive(clap::Subcommand)]
pub enum Action {
    /// Show managed website locations without printing credentials.
    Status,
    /// Show the saved initial OpenList password on the controlling terminal.
    Reveal,
}

fn reveal_initial() -> Result<()> {
    store::check_private(Path::new(INITIAL_PASSWORD), false)?;
    ensure!(
        fs::metadata(INITIAL_PASSWORD)?.len() < 256,
        "Invalid initial OpenList credential record"
    );
    let record = fs::read_to_string(INITIAL_PASSWORD)?;
    let password = record
        .trim()
        .strip_prefix("OPENLIST_ADMIN_PASSWORD=")
        .context("Invalid initial OpenList credential record")?;
    ensure!(
        password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid initial OpenList credential record"
    );
    #[cfg(unix)]
    {
        let mut terminal = fs::OpenOptions::new()
            .write(true)
            .open("/dev/tty")
            .context("A private controlling terminal is required to reveal the initial password")?;
        writeln!(
            terminal,
            "Initial OpenList username: admin\nInitial password (may have been changed in OpenList): {password}"
        )?;
        terminal.flush()?;
        Ok(())
    }
    #[cfg(not(unix))]
    anyhow::bail!("Credential display requires a Linux controlling terminal")
}

pub fn run(store: &Store, action: Action) -> Result<()> {
    crate::host::linux_root()?;
    match action {
        Action::Status => {
            let (c, _) = store.inputs()?;
            deployment_result(&c, false)
        }
        Action::Reveal => reveal_initial(),
    }
}

pub(crate) fn deployment_result(c: &Config, first_display: bool) -> Result<()> {
    if !c.serves_web() {
        return Ok(());
    }
    let port = if c.ports.https == 443 {
        String::new()
    } else {
        format!(":{}", c.ports.https)
    };
    println!(
        "Speedtest: https://{}{port}/\nLibreSpeed: https://{}{port}/librespeed/\nFiles: https://{}{port}/\nManage: nodectl web status | nodectl web reveal",
        c.node.domain, c.node.domain, c.node.reality_domain
    );
    if Path::new(INITIAL_PASSWORD).exists() {
        println!(
            "Protected initial OpenList credential: {INITIAL_PASSWORD} (existing administrator changes remain authoritative)"
        );
        let shown = Path::new("/etc/nodectl-services/openlist-password-shown");
        if first_display && !shown.exists() && reveal_initial().is_ok() {
            store::atomic_write(shown, b"shown\n")?;
        }
    } else {
        println!(
            "Existing OpenList administrator credentials retained; no new password generated."
        );
    }
    Ok(())
}

pub(crate) fn begin_maintenance(store: &Store, backup: &Path) -> Result<()> {
    crate::host::linux_root()?;
    let file = Path::new(MAINTENANCE_FILE);
    trusted_parents(file.parent().unwrap())?;
    public_directory(file.parent().unwrap())?;
    let expected = json!({"schema_version":1,"state_dir":store.state_dir,"backup":backup});
    if file.exists() {
        store::check_private(file, false)?;
        let owner: Value = serde_json::from_slice(&fs::read(file)?)
            .map_err(|_| anyhow::anyhow!("Invalid website maintenance owner"))?;
        ensure!(
            owner == expected,
            "Another website maintenance operation requires recovery first"
        );
    } else {
        store::atomic_write(file, &serde_json::to_vec(&expected)?)?;
    }
    Ok(())
}

pub(crate) fn finish_maintenance(store: &Store, backup: &Path) -> Result<()> {
    let file = Path::new(MAINTENANCE_FILE);
    if !file.exists() {
        return Ok(());
    }
    store::check_private(file, false)?;
    let owner: Value = serde_json::from_slice(&fs::read(file)?)
        .map_err(|_| anyhow::anyhow!("Invalid website maintenance owner"))?;
    ensure!(
        owner == json!({"schema_version":1,"state_dir":store.state_dir,"backup":backup}),
        "Website maintenance ownership differs; retained for inspection"
    );
    fs::remove_file(file)?;
    fs::File::open(file.parent().unwrap())?.sync_all()?;
    Ok(())
}

fn trusted_parents(path: &Path) -> Result<()> {
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Website paths cannot traverse symlink directories"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
                "Website paths require root-owned parents not writable by others"
            );
        }
    }
    Ok(())
}

fn private_tree_without_links(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "OpenList private data cannot contain symlinks during root preparation"
    );
    if metadata.is_dir() {
        for item in fs::read_dir(path)? {
            private_tree_without_links(&item?.path())?;
        }
    } else {
        ensure!(
            metadata.is_file(),
            "OpenList private data contains a special file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.nlink() == 1,
                "OpenList private data cannot contain hardlinks"
            );
        }
    }
    Ok(())
}

pub(crate) fn prepare(c: &Config) -> Result<()> {
    if !c.serves_web() {
        return Ok(());
    }
    crate::host::linux_root()?;
    let web = c.web_services.as_ref().unwrap();
    for user in ["nodectl-speed", "nodectl-files"] {
        if crate::host::command("id", &[user]).is_err() {
            crate::host::command(
                "useradd",
                &[
                    "--system",
                    "--no-create-home",
                    "--shell",
                    "/usr/sbin/nologin",
                    user,
                ],
            )?;
        }
        let record = crate::host::command("getent", &["passwd", user])?;
        let fields: Vec<_> = record.trim().split(':').collect();
        ensure!(
            fields.len() == 7
                && fields[2].parse::<u32>().is_ok_and(|id| id > 0 && id < 1000)
                && ["/usr/sbin/nologin", "/sbin/nologin", "/bin/false"].contains(&fields[6]),
            "Website service account must be a dedicated non-login system user"
        );
    }
    store::private_dir(Path::new("/etc/nodectl-services"))?;
    for directory in [&web.openlist_data_dir, &web.openlist_files_dir] {
        let path = Path::new(directory);
        trusted_parents(path)?;
        if !path.exists() {
            store::private_dir(path)?;
        }
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "OpenList directory cannot be a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.mode() & 0o077 == 0,
                "OpenList data and storage directories must be private"
            );
        }
    }
    private_tree_without_links(Path::new(&web.openlist_data_dir))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let expected = crate::host::command("id", &["-u", "nodectl-files"])?
            .trim()
            .parse::<u32>()?;
        let path = Path::new(&web.openlist_files_dir);
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.uid() == expected
                || (metadata.uid() == 0 && fs::read_dir(path)?.next().is_none()),
            "Existing nonempty file storage must already have access assigned to nodectl-files; data retained"
        );
    }
    let path = Path::new(&web.openlist_data_dir).join("config.json");
    let mut config: Value = if path.exists() {
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() < 1024 * 1024,
            "Unsafe OpenList configuration"
        );
        serde_json::from_slice(&fs::read(&path)?)
            .map_err(|_| anyhow::anyhow!("Invalid OpenList configuration"))?
    } else {
        json!({"database":{"type":"sqlite3","db_file":Path::new(&web.openlist_data_dir).join("data.db"),"table_prefix":"x_"},
        "log":{"enable":false},"temp_dir":Path::new(&web.openlist_data_dir).join("temp"),
        "s3":{"enable":false},"ftp":{"enable":false},"sftp":{"enable":false},"mcp":{"enable":false}})
    };
    ensure!(
        config.is_object(),
        "OpenList configuration must be an object"
    );
    ensure!(
        config["scheme"].is_null() || config["scheme"].is_object(),
        "Invalid OpenList listener configuration"
    );
    ensure!(
        config["database"]["type"] == "sqlite3",
        "Automatic recovery requires a local SQLite OpenList database; external databases require operator-managed recovery"
    );
    let database = Path::new(
        config["database"]["db_file"]
            .as_str()
            .context("OpenList database path missing")?,
    );
    ensure!(
        database.is_absolute()
            && database.parent() == Some(Path::new(&web.openlist_data_dir))
            && crate::config::abs_path(database.to_str().unwrap()),
        "OpenList database must remain in its private data directory"
    );
    for service in ["s3", "ftp", "sftp", "mcp"] {
        ensure!(
            !config[service]["enable"].as_bool().unwrap_or(false),
            "Additional OpenList listeners require separate operator management; configuration retained"
        );
    }
    config["scheme"]["address"] = json!("127.0.0.1");
    config["scheme"]["http_port"] = json!(web.openlist_port);
    config["scheme"]["https_port"] = json!(-1);
    config["scheme"]["force_https"] = json!(false);
    config["scheme"]["unix_file"] = json!("");
    config["scheme"]["enable_h3"] = json!(false);
    store::atomic_write(&path, &serde_json::to_vec_pretty(&config)?)?;
    // The pinned CLI initializes metadata without starting a public server or
    // any offline-download clients. Its output remains private to this process.
    let admin = crate::host::command(
        "/usr/local/bin/openlist",
        &["admin", "--data", &web.openlist_data_dir],
    )?;
    if !admin
        .lines()
        .any(|line| line.starts_with("Admin user's username:"))
        && !Path::new(INITIAL_PASSWORD).exists()
    {
        let mut bytes = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        store::atomic_write(
            Path::new(INITIAL_PASSWORD),
            format!("OPENLIST_ADMIN_PASSWORD={}\n", hex::encode(bytes)).as_bytes(),
        )?;
    }
    if Path::new(INITIAL_PASSWORD).exists() {
        store::check_private(Path::new(INITIAL_PASSWORD), false)?;
    }
    crate::host::command(
        "chown",
        &[
            "--recursive",
            "--no-dereference",
            "--",
            "nodectl-files:nodectl-files",
            &web.openlist_data_dir,
        ],
    )?;
    crate::host::command(
        "chown",
        &[
            "--no-dereference",
            "--",
            "nodectl-files:nodectl-files",
            &web.openlist_files_dir,
        ],
    )?;
    Ok(())
}

fn openlist_api(port: u16, path: &str, token: Option<&str>, body: Option<Value>) -> Result<Value> {
    ensure!(
        path.starts_with("/api/") && !path.contains(['\r', '\n']),
        "Invalid OpenList endpoint"
    );
    let mut config = format!(
        "url = {}\nsilent\nshow-error\nmax-time = 10\nnoproxy = \"*\"\n",
        json!(format!("http://127.0.0.1:{port}{path}"))
    );
    if let Some(token) = token {
        ensure!(
            !token.is_empty()
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "Invalid OpenList administrative credential"
        );
        config.push_str(&format!(
            "header = {}\n",
            json!(format!("Authorization: {token}"))
        ));
    }
    let temporary = tempfile::tempdir()?;
    let request = temporary.path().join("request.json");
    if let Some(body) = body {
        store::atomic_write(&request, &serde_json::to_vec(&body)?)?;
        config.push_str(&format!(
            "header = \"Content-Type: application/json\"\ndata-binary = {}\n",
            json!(format!("@{}", request.display()))
        ));
    }
    let response = crate::host::run(
        "curl",
        &["--disable", "--config", "-"],
        Some(config.as_bytes()),
    )?;
    serde_json::from_slice(&response).map_err(|_| anyhow::anyhow!("Invalid OpenList API response"))
}

pub(crate) fn initialize_storage(c: &Config) -> Result<()> {
    if !c.serves_web() {
        return Ok(());
    }
    let web = c.web_services.as_ref().unwrap();
    let marker = Path::new("/etc/nodectl-services/openlist-storage.json");
    if marker.exists() {
        store::check_private(marker, false)?;
        let saved: Value = serde_json::from_slice(&fs::read(marker)?)
            .map_err(|_| anyhow::anyhow!("Invalid OpenList storage ownership record"))?;
        ensure!(
            saved == json!({"data_dir":web.openlist_data_dir,"files_dir":web.openlist_files_dir}),
            "OpenList data directory migration requires an explicit operator data migration"
        );
        return Ok(());
    }
    let output = crate::host::command(
        "/usr/local/bin/openlist",
        &["admin", "token", "--data", &web.openlist_data_dir],
    )?;
    let token = output
        .lines()
        .find_map(|line| line.strip_prefix("Admin token: "))
        .context("Unable to retrieve private OpenList administrative credential")?;
    let response = openlist_api(
        web.openlist_port,
        "/api/admin/storage/list",
        Some(token),
        None,
    )?;
    ensure!(
        response["code"] == 200,
        "OpenList storage inventory refused"
    );
    let storage = response["data"]["content"]
        .as_array()
        .context("OpenList storage inventory is incomplete")?;
    if storage.is_empty() {
        let response = openlist_api(
            web.openlist_port,
            "/api/admin/storage/create",
            Some(token),
            Some(json!({
                "mount_path":"/","driver":"Local","addition":json!({"root_folder_path":web.openlist_files_dir}).to_string(),
                "web_proxy":true,"enable_sign":true,"cache_expiration":0
            })),
        )?;
        ensure!(
            response["code"] == 200,
            "OpenList local storage creation failed"
        );
    }
    store::atomic_write(
        marker,
        &serde_json::to_vec(
            &json!({"data_dir":web.openlist_data_dir,"files_dir":web.openlist_files_dir}),
        )?,
    )
}

pub(crate) fn health(c: &Config) -> Result<()> {
    if !c.serves_web() {
        return Ok(());
    }
    let web = c.web_services.as_ref().unwrap();
    let started = std::time::Instant::now();
    loop {
        if openlist_api(web.openlist_port, "/api/public/settings", None, None)
            .is_ok_and(|r| r["code"] == 200)
        {
            break;
        }
        ensure!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "OpenList loopback backend did not become ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    crate::host::command(
        "curl",
        &[
            "--disable",
            "--noproxy",
            "*",
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "5",
            &format!(
                "http://127.0.0.1:{}/librespeed/empty.php",
                web.librespeed_port
            ),
        ],
    )?;
    Ok(())
}

pub(crate) fn public_health(c: &Config) -> Result<()> {
    if !c.serves_web() {
        return Ok(());
    }
    for (domain, suffix) in [
        (c.node.domain.as_str(), "/"),
        (c.node.domain.as_str(), "/librespeed/"),
        (c.node.domain.as_str(), "/librespeed/speedtest.js"),
        (c.node.domain.as_str(), "/librespeed/speedtest_worker.js"),
        (c.node.reality_domain.as_str(), "/api/public/settings"),
    ] {
        let port = c.ports.https;
        let maintenance = suffix == "/api/public/settings" && Path::new(MAINTENANCE_FILE).exists();
        let status = crate::host::command(
            "curl",
            &[
                "--disable",
                "--noproxy",
                "*",
                "--silent",
                "--show-error",
                "--max-time",
                "8",
                "--resolve",
                &format!("{domain}:{port}:127.0.0.1"),
                "--output",
                "/dev/null",
                "--write-out",
                "%{http_code}",
                &format!("https://{domain}:{port}{suffix}"),
            ],
        )?;
        ensure!(
            status == if maintenance { "503" } else { "200" },
            "Managed public website failed HTTPS readiness: {domain}{suffix}"
        );
    }
    Ok(())
}

fn public_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir(path)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Unsafe public website directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
            "Public website directories must be owned by the operator and not writable by others"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub(crate) fn check_public_asset(path: &Path, digest: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Published asset must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o777 == 0o644
                && metadata.nlink() == 1,
            "Published assets require operator ownership, 0644 and a single link"
        );
    }
    ensure!(
        store::file_digest(path)? == digest,
        "Published asset digest differs from its immutable candidate"
    );
    Ok(())
}

fn public_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        return check_public_asset(path, &store::digest(bytes));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

/// The only files copied out of a private release are this static whitelist.
/// No config, client credentials, certificate or server private key is public.
pub(crate) fn publish_assets(store: &Store, id: &str) -> Result<PathBuf> {
    crate::host::linux_root()?;
    let (config, _) = store.verify(id)?;
    ensure!(config.serves_web(), "Website service is disabled");
    let root = Path::new(PUBLIC_ROOT);
    trusted_parents(root)?;
    let release = store.release(id)?;
    let directory = publish_at(root, id, &release)?;
    prepare_download(
        root,
        u64::from(config.web_services.as_ref().unwrap().download_size_mib) * 1024 * 1024,
    )?;
    Ok(directory)
}

fn prepare_download(root: &Path, length: u64) -> Result<()> {
    public_directory(&root.join("downloads"))?;
    let destination = root.join("downloads/test.bin");
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Test download cannot replace a symlink or directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == 0 && metadata.mode() & 0o022 == 0 && metadata.nlink() == 1,
                "Unsafe test download ownership"
            );
        }
        if metadata.len() == length {
            return Ok(());
        }
    }
    #[cfg(unix)]
    {
        let name = std::ffi::CString::new(root.as_os_str().as_encoded_bytes())?;
        let mut space = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        ensure!(
            unsafe { libc::statvfs(name.as_ptr(), space.as_mut_ptr()) } == 0,
            "Cannot verify download disk space"
        );
        let space = unsafe { space.assume_init() };
        ensure!(
            u128::from(space.f_bavail) * u128::from(space.f_frsize)
                > u128::from(length) + 64 * 1024 * 1024,
            "Insufficient free space for the configured test download"
        );
    }
    let temporary = tempfile::NamedTempFile::new_in(root.join("downloads"))?;
    temporary.as_file().set_len(length)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(destination)?;
    Ok(())
}

fn publish_at(root: &Path, id: &str, release: &Path) -> Result<PathBuf> {
    publish_nonsecret_assets(
        root,
        id,
        &release.join("website"),
        &["index.html", "site.css", "speedtest.html", "test.js"],
        b"nodectl-public-assets-v1\n",
    )
}

/// Publish only an explicit nonsecret whitelist. The private snapshot and its
/// permissions remain unchanged; large public datasets need no credentials.
pub(crate) fn publish_nonsecret_assets(
    root: &Path,
    id: &str,
    source: &Path,
    names: &[&str],
    marker: &[u8],
) -> Result<PathBuf> {
    trusted_parents(root)?;
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid public release identity"
    );
    if root.exists() && !root.join(".nodectl-owned").exists() {
        ensure!(
            fs::read_dir(root)?.next().is_none(),
            "Refusing to adopt an existing public website directory"
        );
    }
    public_directory(root)?;
    if !root.join(".nodectl-owned").exists() {
        store::atomic_write(&root.join(".nodectl-owned"), marker)?;
    }
    store::check_private(&root.join(".nodectl-owned"), false)?;
    ensure!(
        fs::read(root.join(".nodectl-owned"))? == marker,
        "Unexpected public website ownership record"
    );
    public_directory(&root.join("releases"))?;
    let directory = root.join("releases").join(id);
    public_directory(&directory)?;
    for name in names {
        ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'.'),
            "Invalid nonsecret asset name"
        );
        public_file(&directory.join(name), &fs::read(source.join(name))?)?;
    }
    let actual = fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        actual.len() == names.len(),
        "Unexpected file in public website release"
    );
    fs::File::open(&directory)?.sync_all()?;
    Ok(directory)
}

pub fn nginx(c: &Config, vars: &mut BTreeMap<&'static str, String>) -> Result<()> {
    let transports =
        render::template(include_str!("../templates/nginx-transports.conf.tpl"), vars)?;
    vars.insert("TRANSPORT_ROUTES", transports);
    vars.insert("WEB_ROUTES", String::new());
    vars.insert("FILES_SERVER", String::new());
    vars.insert("WEB_LIMITS", String::new());
    if !c.serves_web() {
        return Ok(());
    }
    let Some(web) = c.web_services.as_ref().filter(|web| web.enabled) else {
        return Ok(());
    };
    vars.insert(
        "DOMAINS",
        c.domains()
            .into_iter()
            .filter(|n| n != &c.node.reality_domain)
            .collect::<Vec<_>>()
            .join(" "),
    );
    // OpenList's signed download URLs also need query-free safe logs.
    vars.insert("SHARE_LOG_FORMAT", "log_format node_safe '$remote_addr [$time_local] \"$request_method $uri\" $status $body_bytes_sent';".into());
    vars.insert("SHARE_LOG_TYPE", "node_safe".into());
    vars.insert("SHARE_ERROR", "error_log /dev/null;".into());
    vars.insert("WEB_LIMITS", "limit_conn_zone $binary_remote_addr zone=website_connections:10m;\n    limit_req_zone $binary_remote_addr zone=website_auth:10m rate=2r/s;".into());
    vars.insert("WEB_ROUTES", format!(r#"
        location = / {{ root {PUBLIC_ROOT}/current; try_files /index.html =404; default_type text/html; }}
        location = /site.css {{ alias {PUBLIC_ROOT}/current/site.css; default_type text/css; }}
        location = /librespeed/ {{ root {PUBLIC_ROOT}/current; try_files /speedtest.html =404; default_type text/html; add_header Cache-Control no-store always; }}
        location = /librespeed/test.js {{ alias {PUBLIC_ROOT}/current/test.js; default_type application/javascript; }}
        location = /librespeed/getIP.php {{ return 404; }}
        location = /librespeed/getIP {{ return 404; }}
        location = /librespeed/backend/getIP.php {{ return 404; }}
        location = /librespeed/backend/getIP {{ return 404; }}
        location ^~ /librespeed/ {{
            limit_conn website_connections 32;
            limit_conn_status 429;
            proxy_pass http://127.0.0.1:{};
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $remote_addr;
            proxy_request_buffering off;
            proxy_buffering off;
            proxy_max_temp_file_size 0;
            client_max_body_size 64m;
            client_body_timeout 60s;
            proxy_read_timeout 60s;
            proxy_send_timeout 60s;
            gzip off;
            add_header Cache-Control no-store always;
        }}
        location = /downloads/test.bin {{
            limit_conn website_connections 8;
            limit_conn_status 429;
            alias {PUBLIC_ROOT}/downloads/test.bin;
            gzip off;
            max_ranges 1;
            add_header Content-Disposition 'attachment; filename="test.bin"';
        }}"#, web.librespeed_port));
    // Keep authenticated subscriptions on both origins, but isolate the node UI
    // from user-supplied file content. Do not change the frozen v3 route helper.
    let files_share = if c.distributes() {
        let port = if c.ports.https == 443 {
            String::new()
        } else {
            format!(":{}", c.ports.https)
        };
        let mut routes = format!(
            "location = /nodes/ {{ return 302 https://{}{port}/nodes/; }}\n        location ^~ /nodes/ {{ return 404; }}",
            c.node.domain
        );
        for format in ["xray", "mihomo"] {
            routes.push('\n');
            routes.push_str(&crate::share_http::nginx_route().replacen(
                "location ^~ /nodes/",
                &format!("location = /nodes/sub/{format}"),
                1,
            ));
        }
        routes
    } else {
        String::new()
    };
    let server = format!(
        r#"server {{
{{{{ DOMAIN_LISTEN }}}}
        server_name {};
        error_log /dev/null;
        ssl_certificate {{{{ DOMAIN_CERT }}}}/fullchain.pem;
        ssl_certificate_key {{{{ DOMAIN_CERT }}}}/privkey.pem;
        {{{{ ALT_SVC }}}}
        {files_share}
{{{{ TRANSPORT_ROUTES }}}}
        location / {{
            if (-f /var/lib/nodectl-web-guard/active) {{ return 503; }}
            limit_conn website_connections 32;
            limit_conn_status 429;
            proxy_pass http://127.0.0.1:{openlist_port};
            proxy_http_version 1.1;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $remote_addr;
            proxy_set_header X-Forwarded-Proto $scheme;
            client_max_body_size 0;
            client_body_timeout 1h;
            proxy_request_buffering off;
            proxy_buffering off;
            proxy_max_temp_file_size 0;
            proxy_read_timeout 1h;
            proxy_send_timeout 1h;
            add_header Referrer-Policy no-referrer always;
            add_header X-Content-Type-Options nosniff always;
            location ~ ^/(d|p|ad|ap|ae|sd|sad|dav|s3)(/|$) {{
                if (-f /var/lib/nodectl-web-guard/active) {{ return 503; }}
                proxy_pass http://127.0.0.1:{openlist_port};
                proxy_hide_header Content-Disposition;
                add_header Content-Disposition attachment always;
                add_header Content-Security-Policy "sandbox; default-src 'none'; frame-ancestors 'none'" always;
                add_header X-Content-Type-Options nosniff always;
                add_header Referrer-Policy no-referrer always;
            }}
            location ~ ^/api/auth/(login|login/hash|login/ldap)$ {{
                if (-f /var/lib/nodectl-web-guard/active) {{ return 503; }}
                proxy_pass http://127.0.0.1:{openlist_port};
                limit_req zone=website_auth burst=5 nodelay;
                limit_req_status 429;
            }}
        }}
    }}"#,
        c.node.reality_domain,
        openlist_port = web.openlist_port
    );
    vars.insert("FILES_SERVER", render::template(&server, vars)?);
    Ok(())
}

pub fn files(c: &Config, files: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
    let Some(web) = c.web_services.as_ref().filter(|web| web.enabled) else {
        return Ok(());
    };
    let port = if c.ports.https == 443 {
        String::new()
    } else {
        format!(":{}", c.ports.https)
    };
    let vars = BTreeMap::from([
        (
            "FILES_URL",
            format!("https://{}{port}/", c.node.reality_domain),
        ),
        ("DOWNLOAD_MIB", web.download_size_mib.to_string()),
        (
            "NODE_LINK",
            if c.distributes() {
                "<a href=\"/nodes/\">节点访问</a>"
            } else {
                ""
            }
            .into(),
        ),
    ]);
    files.insert(
        "website/index.html".into(),
        render::template(include_str!("../assets/website/index.html"), &vars)?.into_bytes(),
    );
    for (name, bytes) in [
        (
            "site.css",
            include_bytes!("../assets/website/site.css").as_slice(),
        ),
        (
            "speedtest.html",
            include_bytes!("../assets/website/speedtest.html").as_slice(),
        ),
        (
            "test.js",
            include_bytes!("../assets/website/test.js").as_slice(),
        ),
    ] {
        files.insert(format!("website/{name}"), bytes.to_vec());
    }
    files.insert("web/librespeed.toml".into(), format!(
        "bind_address=\"127.0.0.1\"\nlisten_port={}\nurl_base=\"/librespeed\"\nproxyprotocol_port=0\nserver_lat=1\nserver_lng=1\ndatabase_type=\"none\"\nstatistics_password=\"\"\nredact_ip_addresses=true\nenable_tls=false\nenable_http2=false\n", web.librespeed_port).into_bytes());
    files.insert(
        "systemd/librespeed.service".into(),
        include_bytes!("../templates/librespeed.service.tpl").to_vec(),
    );
    let vars = BTreeMap::from([
        ("OPENLIST_DATA", web.openlist_data_dir.clone()),
        ("OPENLIST_FILES", web.openlist_files_dir.clone()),
    ]);
    files.insert(
        "systemd/openlist.service".into(),
        render::template(include_str!("../templates/openlist.service.tpl"), &vars)?.into_bytes(),
    );
    Ok(())
}
