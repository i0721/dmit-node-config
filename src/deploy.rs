//! DMIT orchestration using the existing Config, renderer, receipts and transactions.
use crate::{
    config::{self, Config, Secrets},
    dns::{self, Change, Cloudflare},
    host, provision,
    store::{self, Store},
};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Args)]
pub struct Deploy {
    /// Optional complete node.toml candidate; existing secrets are always retained.
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// Root domain for new DMIT names (for example example.com).
    #[arg(long)]
    pub domain_suffix: Option<String>,
    #[arg(long, default_value = "dmit-")]
    pub subdomain_prefix: String,
    #[arg(long)]
    pub domain: Option<String>,
    #[arg(long)]
    pub reality_domain: Option<String>,
    #[arg(long, value_delimiter = ',')]
    pub aliases: Option<Vec<String>>,
    #[arg(long)]
    pub ipv4: Option<String>,
    /// Empty string explicitly disables IPv6.
    #[arg(long)]
    pub ipv6: Option<String>,
    #[arg(long)]
    pub http_port: Option<u16>,
    #[arg(long)]
    pub https_port: Option<u16>,
    #[arg(long)]
    pub reality_port: Option<u16>,
    #[arg(long,action=clap::ArgAction::Set)]
    pub warp: Option<bool>,
    #[arg(long)]
    pub non_interactive: bool,
    /// Keep dependency provisioning manual.
    #[arg(long)]
    pub skip_dependencies: bool,
    /// Omit for manual DNS; public DNS validation still runs.
    #[arg(long)]
    pub dns_zone: Option<String>,
    /// Owner-only file, never a token on argv. Otherwise read from /dev/tty without echo.
    #[arg(long)]
    pub token_file: Option<PathBuf>,
    #[arg(long)]
    pub manual_certificates: bool,
    #[arg(long)]
    pub acme_email: Option<String>,
    /// Let acme.sh discover zones for SANs spanning retained legacy domains.
    #[arg(long)]
    pub acme_discover_zones: bool,
    #[arg(long)]
    pub accept_ca_terms: bool,
    #[arg(long)]
    pub accept_warp_terms: bool,
    /// Explicit consent to back up and replace unmanaged entrypoints.
    #[arg(long)]
    pub takeover: bool,
    #[arg(long, default_value = "https://www.cloudflare.com/cdn-cgi/trace")]
    pub probe_url: String,
    /// Wait for public A/AAAA propagation before failing and restoring DNS.
    #[arg(long,default_value_t=300,value_parser=clap::value_parser!(u64).range(1..=900))]
    pub dns_wait_seconds: u64,
    /// Execute only the exact preview with this digest.
    #[arg(long)]
    pub approve: Option<String>,
    #[arg(long)]
    pub execute: bool,
}
pub(crate) fn preview_secrets() -> Secrets {
    Secrets {
        xhttp_uuid: "10a6cfe1-d381-4c46-865e-998b28b801a1".into(),
        ws_uuid: "20a6cfe1-d381-4c46-865e-998b28b801a2".into(),
        reality_uuid: "30a6cfe1-d381-4c46-865e-998b28b801a3".into(),
        reality_private_key: "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc".into(),
        reality_short_id: "0123456789abcdef".into(),
    }
}
fn prompt(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    ensure!(
        io::stdin().read_line(&mut input)? > 0,
        "Input ended while reading {label}"
    );
    Ok(if input.trim().is_empty() {
        default.into()
    } else {
        input.trim().into()
    })
}
pub(crate) fn secret_prompt(label: &str) -> Result<String> {
    #[cfg(unix)]
    {
        use std::{
            io::{BufRead, BufReader},
            os::fd::AsRawFd,
        };
        let mut tty = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("Credential input requires a terminal or --token-file")?;
        let fd = tty.as_raw_fd();
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        ensure!(
            unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } == 0,
            "Cannot read terminal mode"
        );
        let original = unsafe { original.assume_init() };
        struct Echo(i32, libc::termios);
        impl Drop for Echo {
            fn drop(&mut self) {
                unsafe { libc::tcsetattr(self.0, libc::TCSANOW, &self.1) };
            }
        }
        let guard = Echo(fd, original);
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        ensure!(
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } == 0,
            "Cannot disable credential echo"
        );
        write!(tty, "{label}: ")?;
        tty.flush()?;
        let mut input = String::new();
        ensure!(
            BufReader::new(&tty).read_line(&mut input)? > 0,
            "Credential input ended"
        );
        writeln!(tty)?;
        drop(guard);
        Ok(input.trim().into())
    }
    #[cfg(not(unix))]
    {
        let _ = label;
        bail!("Secure terminal credential input requires Linux; use --token-file");
    }
}
fn detected_ip(v6: bool) -> Result<String> {
    let data = host::command(
        "ip",
        &[
            if v6 { "-6" } else { "-4" },
            "-j",
            "address",
            "show",
            "scope",
            "global",
        ],
    )?;
    let interfaces: Vec<Value> = serde_json::from_str(&data)?;
    let mut found = vec![];
    for interface in interfaces {
        let name = interface["ifname"].as_str().unwrap_or("");
        if ["tun", "warp", "docker", "br-", "veth"]
            .iter()
            .any(|p| name.starts_with(p))
        {
            continue;
        }
        if let Some(addresses) = interface["addr_info"].as_array() {
            for ip in addresses {
                if let Some(ip) = ip["local"].as_str() {
                    found.push(ip.to_owned());
                }
            }
        }
    }
    found.sort();
    found.dedup();
    ensure!(
        found.len() <= 1,
        "Multiple global IPs detected; specify address explicitly"
    );
    Ok(found.pop().unwrap_or_default())
}
pub(crate) fn candidate(store: &Store, args: &Deploy) -> Result<(Config, Secrets, bool)> {
    let exists = store.config_dir.exists();
    let (mut c, s) = if exists {
        store.inputs()?
    } else {
        (toml::from_str(config::EXAMPLE)?, preview_secrets())
    };
    if let Some(input) = &args.input {
        store::check_private(input, false)?;
        c = toml::from_str(&fs::read_to_string(input)?)
            .map_err(|_| anyhow::anyhow!("Invalid candidate node.toml"))?;
    }
    if !exists && args.input.is_none() {
        let suffix = match &args.domain_suffix {
            Some(v) => v.clone(),
            None if args.domain.is_some() => args
                .domain
                .as_ref()
                .and_then(|v| v.split_once('.').map(|(_, suffix)| suffix.to_owned()))
                .context("Domain must include a suffix")?,
            None if args.non_interactive => {
                bail!("New deployment requires --domain-suffix or --domain")
            }
            None => prompt("Root domain", "")?,
        };
        if !suffix.is_empty() {
            ensure!(config::domain(&suffix), "Invalid root domain");
            c.node.domain = format!("{}qwq1.{suffix}", args.subdomain_prefix);
            c.node.reality_domain = format!("{}reality.{suffix}", args.subdomain_prefix);
            c.node.aliases = vec![format!("{}www.{suffix}", args.subdomain_prefix)];
        }
        c.node.ipv4 = if args.ipv4.is_some() {
            String::new()
        } else {
            detected_ip(false)?
        };
        c.node.ipv6 = if args.ipv6.is_some() {
            String::new()
        } else {
            detected_ip(true)?
        };
        // Optional WARP defaults to the repository baseline, not a simplified profile.
    }
    macro_rules! set {
        ($field:expr,$arg:expr) => {
            if let Some(v) = &$arg {
                $field = v.clone();
            }
        };
    }
    set!(c.node.domain, args.domain);
    set!(c.node.reality_domain, args.reality_domain);
    set!(c.node.aliases, args.aliases);
    set!(c.node.ipv4, args.ipv4);
    set!(c.node.ipv6, args.ipv6);
    if let Some(v) = args.http_port {
        c.ports.http = v;
    }
    if let Some(v) = args.https_port {
        c.ports.https = v;
    }
    if let Some(v) = args.reality_port {
        c.ports.reality = v;
    }
    if let Some(v) = args.warp {
        c.warp.enabled = v;
    }
    if !args.non_interactive {
        c.node.domain = prompt("TLS domain", &c.node.domain)?;
        c.node.reality_domain = prompt("REALITY domain", &c.node.reality_domain)?;
        let aliases = prompt(
            "Aliases (comma separated; '-' clears)",
            &c.node.aliases.join(","),
        )?;
        c.node.aliases = if aliases == "-" {
            vec![]
        } else {
            aliases
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_owned())
                .collect()
        };
        c.node.ipv4 = prompt("Public IPv4", &c.node.ipv4)?;
        let ip6 = prompt("Public IPv6 ('-' disables)", &c.node.ipv6)?;
        c.node.ipv6 = if ip6 == "-" { String::new() } else { ip6 };
        c.ports.http = prompt("HTTP port", &c.ports.http.to_string())?.parse()?;
        c.ports.https = prompt("HTTPS port", &c.ports.https.to_string())?.parse()?;
        c.ports.reality = prompt("REALITY port", &c.ports.reality.to_string())?.parse()?;
        c.warp.enabled =
            prompt("Independent WARP (true/false)", &c.warp.enabled.to_string())?.parse()?;
    }
    config::validate(&c, &s)?;
    if exists {
        let (old, _) = store.inputs()?;
        // Preserve old TLS and REALITY names when changing the primary domain.
        for domain in [&old.node.domain, &old.node.reality_domain] {
            if domain != &c.node.domain
                && domain != &c.node.reality_domain
                && !c.node.aliases.contains(domain)
            {
                c.node.aliases.push(domain.clone());
            }
        }
        config::validate(&c, &s)?;
        ensure!(
            old.runtime.xray_binary == c.runtime.xray_binary
                && old.runtime.nginx_binary == c.runtime.nginx_binary
                && old.runtime.usque_binary == c.runtime.usque_binary,
            "Runtime path migration requires manual provisioning"
        );
    }
    Ok((c, s, exists))
}
fn preserves_clients(old: &Config, old_s: &Secrets, new: &Config, new_s: &Secrets) -> Result<bool> {
    Ok(old.ports.https == new.ports.https
        && old.ports.reality == new.ports.reality
        && old.transport.xhttp.path == new.transport.xhttp.path
        && old.transport.ws.path == new.transport.ws.path
        && old.transport.reality.path == new.transport.reality.path
        && old
            .domains()
            .iter()
            .all(|name| new.domains().contains(name))
        && serde_json::to_value(old_s)? == serde_json::to_value(new_s)?)
}
fn fingerprint(path: &Path) -> Result<Value> {
    fn attributes(mut value: Value, m: &fs::Metadata) -> Value {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            value["mode"] = json!(m.mode() & 0o7777);
            value["uid"] = json!(m.uid());
            value["gid"] = json!(m.gid());
        }
        #[cfg(not(unix))]
        let _ = m;
        value
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Ok(attributes(json!({"link":fs::read_link(path)?}), &m))
        }
        Ok(m) if m.is_file() => Ok(attributes(json!({"sha256":store::file_digest(path)?}), &m)),
        Ok(m) if m.is_dir() => {
            let mut children = BTreeMap::new();
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                children.insert(
                    entry.file_name().to_string_lossy().to_string(),
                    fingerprint(&entry.path())?,
                );
            }
            Ok(attributes(
                json!({"tree_sha256":store::digest(&serde_json::to_vec(&children)?)}),
                &m,
            ))
        }
        Ok(_) => bail!("Unsupported deployment backup file: {}", path.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(e.into()),
    }
}
fn backup_paths(store: &Store, c: &Config) -> Vec<PathBuf> {
    let mut paths = vec![
        store.config_dir.clone(),
        PathBuf::from(&c.tls.domain_cert_dir),
        PathBuf::from(&c.tls.ip_cert_dir),
        PathBuf::from(&c.warp.config_file),
        PathBuf::from(&c.runtime.xray_binary),
        PathBuf::from(&c.runtime.nginx_binary),
        PathBuf::from(&c.runtime.usque_binary),
        PathBuf::from("/root/.acme.sh"),
        PathBuf::from("/etc/apt/sources.list.d/nginx.list"),
        PathBuf::from("/usr/share/keyrings/nginx-archive-keyring.gpg"),
        PathBuf::from("/usr/sbin/policy-rc.d"),
        PathBuf::from("/usr/local/go"),
        PathBuf::from("/var/spool/cron/crontabs/root"),
    ];
    paths.sort();
    paths.dedup();
    paths
}
fn inventory(store: &Store, c: &Config) -> Result<Value> {
    ensure!(
        cfg!(target_os = "linux"),
        "Deployment planning requires Linux/systemd"
    );
    let os = fs::read_to_string("/etc/os-release")?;
    ensure!(
        os.lines()
            .any(|l| ["ID=debian", "ID=ubuntu", "ID=\"debian\"", "ID=\"ubuntu\""].contains(&l)),
        "Automatic deployment supports Debian/Ubuntu only; manual mode remains available"
    );
    host::command("systemctl", &["show-environment"])?;
    let addresses: Value = serde_json::from_str(&host::command("ip", &["-j", "address", "show"])?)?;
    for wanted in c.ips() {
        ensure!(
            addresses
                .as_array()
                .context("Host interfaces")?
                .iter()
                .any(|v| v["addr_info"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v["local"]
                        .as_str()
                        .and_then(|s| s.parse::<std::net::IpAddr>().ok())
                        == wanted.parse::<std::net::IpAddr>().ok()))),
            "Configured IP is not assigned to this DMIT host; correct inputs before deployment"
        );
    }
    let current = store.pointer("current")?;
    let mut files = BTreeMap::new();
    for path in backup_paths(store, c).into_iter().chain(
        host::ENTRIES
            .into_iter()
            .map(|(path, _)| PathBuf::from(path)),
    ) {
        files.insert(path.display().to_string(), fingerprint(&path)?);
    }
    files.insert(
        c.runtime.nginx_binary.clone(),
        fingerprint(Path::new(&c.runtime.nginx_binary))?,
    );
    let mut binary_digests = BTreeMap::new();
    for path in [
        &c.runtime.xray_binary,
        &c.runtime.nginx_binary,
        &c.runtime.usque_binary,
    ] {
        binary_digests.insert(
            path,
            if Path::new(path).is_file() {
                Some(store::file_digest(Path::new(path))?)
            } else {
                None
            },
        );
    }
    let mut services = BTreeMap::new();
    for service in host::SERVICES {
        services.insert(
            service,
            host::command(
                "systemctl",
                &[
                    "show",
                    service,
                    "-p",
                    "LoadState",
                    "-p",
                    "ActiveState",
                    "-p",
                    "UnitFileState",
                ],
            )?,
        );
    }
    let listeners = host::command("ss", &["-H", "-lntu"])?;
    let mut listeners = listeners.lines().map(str::to_owned).collect::<Vec<_>>();
    listeners.sort();
    Ok(
        json!({"os":os,"architecture":std::env::consts::ARCH,"current":current,"files":files,"binary_digests":binary_digests,"services":services,"listeners":listeners,"routes4":host::default_routes(false)?,"routes6":host::default_routes(true)?}),
    )
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    path: PathBuf,
    copy: PathBuf,
    existed: bool,
    fingerprint: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    backup: PathBuf,
    config: Config,
    previous: Option<String>,
    reference: host::Reference,
    files: Vec<Backup>,
    dns_zone: Option<String>,
    dns: Vec<(Change, Value)>,
    pending_dns: Option<Change>,
    routes4: String,
    routes6: String,
    phase: String,
    config_existed: bool,
    services: BTreeMap<String, host::Service>,
}
fn save(store: &Store, journal: &Journal) -> Result<()> {
    store::atomic_write(
        &store.state_dir.join("deployment.json"),
        &serde_json::to_vec_pretty(journal)?,
    )
}
fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::read_link(source)?, dest)?;
        #[cfg(not(unix))]
        bail!("Deployment backups require Linux");
    } else if metadata.is_dir() {
        fs::create_dir(dest)?;
        for file in fs::read_dir(source)? {
            let file = file?;
            copy_tree(&file.path(), &dest.join(file.file_name()))?;
        }
        fs::set_permissions(dest, metadata.permissions())?;
    } else if metadata.is_file() {
        fs::copy(source, dest)?;
        fs::set_permissions(dest, metadata.permissions())?;
    } else {
        bail!("Cannot back up special file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let name = std::ffi::CString::new(dest.as_os_str().as_bytes())?;
        ensure!(
            unsafe { libc::lchown(name.as_ptr(), metadata.uid(), metadata.gid()) } == 0,
            "Cannot preserve backup ownership"
        );
    }
    Ok(())
}
fn remove(path: &Path) -> Result<()> {
    if let Ok(m) = fs::symlink_metadata(path) {
        if m.is_dir() && !m.file_type().is_symlink() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
fn initialize(store: &Store, c: &Config, explicit_input: bool, work: &Path) -> Result<()> {
    let template = work.join("candidate.toml");
    store::atomic_write(&template, toml::to_string_pretty(c)?.as_bytes())?;
    config::init(&store.config_dir, Some(&template), &[])?;
    if explicit_input {
        store::atomic_write(
            &store.config_dir.join("node.toml"),
            toml::to_string_pretty(c)?.as_bytes(),
        )?;
    }
    store.inputs()?;
    Ok(())
}

pub fn run(store: &Store, mut args: Deploy) -> Result<()> {
    ensure!(
        !store.state_dir.join("deployment.json").exists(),
        "Interrupted provisioning exists; run recover first"
    );
    let (c, s, exists) = candidate(store, &args)?;
    ensure!(
        config::abs_path(store.config_dir.to_str().context("Config path")?)
            && config::abs_path(store.state_dir.to_str().context("State path")?),
        "Automatic deployment requires safe absolute Linux config/state paths"
    );
    let inherited = if exists {
        Some(store.inputs()?.0)
    } else {
        None
    };
    if !args.non_interactive {
        let mode = prompt(
            "DNS mode (manual/cloudflare)",
            if args.dns_zone.is_some() {
                "cloudflare"
            } else {
                "manual"
            },
        )?;
        ensure!(
            ["manual", "cloudflare"].contains(&mode.as_str()),
            "Unknown DNS mode"
        );
        if mode == "cloudflare" {
            args.dns_zone = Some(prompt(
                "Cloudflare Zone ID",
                args.dns_zone.as_deref().unwrap_or(""),
            )?);
        } else {
            args.dns_zone = None;
        }
        args.manual_certificates = prompt(
            "Use manually provisioned certificates (true/false)",
            &args.manual_certificates.to_string(),
        )?
        .parse()?;
        if !args.manual_certificates
            && host::certificates(&c).is_err()
            && !provision::existing_acme_account()
        {
            args.acme_email = Some(prompt(
                "ACME email",
                args.acme_email.as_deref().unwrap_or(""),
            )?);
        }
    }
    let paths = backup_paths(store, &c);
    for path in &paths {
        ensure!(
            path.is_absolute()
                && path.components().count() >= 3
                && !store.state_dir.starts_with(path),
            "Unsafe or overlapping automatic backup path; use a dedicated config/certificate directory"
        );
    }
    let cf = if let Some(zone) = &args.dns_zone {
        Some(Cloudflare::new(
            zone,
            dns::token(args.token_file.as_deref())?,
        )?)
    } else {
        None
    };
    let changes = cf
        .as_ref()
        .map(|cf| cf.plan(&c, inherited.as_ref()))
        .transpose()?
        .unwrap_or_default();
    let inventory = inventory(store, &c)?;
    let previous = store.pointer("current")?;
    let active_config = previous
        .as_ref()
        .map(|id| store.verify(id).map(|(c, _)| c))
        .transpose()?
        .or_else(|| inherited.clone());
    let superseded_acme = active_config
        .as_ref()
        .filter(|old| {
            old.node.domain != c.node.domain && old.tls.domain_cert_dir == c.tls.domain_cert_dir
        })
        .map(|old| old.node.domain.clone());
    let entrypoints_absent = host::ENTRIES
        .iter()
        .all(|(path, _)| inventory["files"][path].is_null());
    let preserve_clients = if let Some(old) = &previous {
        let (old, old_s) = store.verify(old)?;
        preserves_clients(&old, &old_s, &c, &s)?
    } else {
        false
    };
    let domain_cert_valid = host::certificates_part(&c, Some(false)).is_ok();
    let ip_cert_valid = host::certificates_part(&c, Some(true)).is_ok();
    let needs_cert = !domain_cert_valid || !ip_cert_valid;
    let mut required_actions = vec![];
    let mut known_acme = c.sans();
    if let Some(old) = &active_config {
        known_acme.extend(old.sans());
    }
    if !args.manual_certificates && provision::acme_scope(&known_acme).is_err() {
        required_actions.push("ACME home is shared or unreadable; preserve unrelated certificates with --manual-certificates or a dedicated client");
    }
    if previous.is_none() && !entrypoints_absent && !args.takeover {
        required_actions
            .push("Review existing entrypoints and provide --takeover before replacement");
    }
    if args.manual_certificates && needs_cert {
        required_actions.push("Provision valid domain/IP certificates, then preview again");
    }
    if !args.manual_certificates && !domain_cert_valid && cf.is_none() {
        required_actions.push("Provide Cloudflare --dns-zone and a protected token for domain DNS-01, or use manually provisioned certificates");
    }
    if !args.manual_certificates && needs_cert && !provision::existing_acme_account() {
        if args.acme_email.is_none() {
            required_actions.push("Provide --acme-email for the new ACME account");
        }
        if !args.accept_ca_terms {
            required_actions.push("Review CA terms and supply --accept-ca-terms for a new account");
        }
    }
    if c.warp.enabled && !Path::new(&c.warp.config_file).exists() && !args.accept_warp_terms {
        required_actions.push("Review WARP terms and supply --accept-warp-terms for new device registration, or select --warp false");
    }
    let configuration_diff = if let Some(old) = &inherited {
        similar::TextDiff::from_lines(&toml::to_string_pretty(old)?, &toml::to_string_pretty(&c)?)
            .unified_diff()
            .header("existing node.toml", "candidate node.toml")
            .to_string()
    } else {
        "New DMIT configuration; credentials generated on execution".into()
    };
    let plan = json!({"schema_version":1,"configuration_diff":configuration_diff,"required_actions":required_actions,"certificate_status":{"domain_valid":domain_cert_valid,"ip_valid":ip_cert_valid},"configuration":c,"preserve_existing_secrets":exists,"generate_transport_paths":!exists && args.input.is_none(),"previous_client_compatibility":if previous.is_none(){"no_previous_managed_release"}else if preserve_clients{"preserved_and_live_test_required"}else{"explicit_input_changes_require_client_updates"},"secret_digest":if exists{Some(store::digest(&serde_json::to_vec(&s)?))}else{None},"inventory":inventory,"dns_changes":changes,"manual_dns":cf.is_none(),"manual_certificates":args.manual_certificates,"provision_dependencies":!args.skip_dependencies,"takeover":args.takeover,"new_package_defaults_may_be_claimed":entrypoints_absent && !args.skip_dependencies,"probe_url":args.probe_url,"dns_wait_seconds":args.dns_wait_seconds,"acme_email":args.acme_email,"acme_discover_zones":args.acme_discover_zones,"retire_superseded_acme_renewal":if args.manual_certificates{None}else{superseded_acme.as_deref()},"accept_ca_terms":args.accept_ca_terms,"accept_warp_terms":args.accept_warp_terms,"generator":env!("CARGO_PKG_VERSION"),"executable_sha256":store::file_digest(&std::env::current_exe()?)?,"versions_lock_sha256":store::digest(include_bytes!("../versions.lock.toml")),"stages":["backup","dependencies","inputs","dns","certificates","warp","warp-socks-handshake-with-temporary-tun-stop","render","diff","check","takeover-if-authorized","apply-with-live-handshakes","renewal"],"recovery_limits":["Package transactions and CA issuance cannot be undone by a configuration rollback; backups and DNS recovery are retained","Unknown unmanaged service credentials are never inferred: import verified node.toml/secrets.json before takeover"]});
    let digest = store::digest(&serde_json::to_vec(&plan)?);
    println!(
        "{}\nPlan SHA256: {digest}",
        serde_json::to_string_pretty(&plan)?
    );
    if !args.execute {
        println!(
            "Preview only. Review this plan; rerun with the same options --execute --approve {digest}"
        );
        return Ok(());
    }
    ensure!(
        args.approve.as_deref() == Some(&digest),
        "Plan approval missing or stale; review a fresh preview"
    );
    ensure!(
        required_actions.is_empty(),
        "Plan has unmet prerequisites; resolve required_actions and preview again"
    );
    host::linux_root()?;
    let _lock = store.lock()?;
    let _lease = provision::Lease::hold(&_lock);
    ensure!(
        inventory == self::inventory(store, &c)?,
        "Host changed while acquiring lock; preview again"
    );
    if let Some(cf) = &cf {
        ensure!(
            serde_json::to_vec(&changes)? == serde_json::to_vec(&cf.plan(&c, inherited.as_ref())?)?,
            "DNS changed after preview"
        );
    }
    // Refuse a fresh deployment over live legacy service identities without imported inputs.
    if !exists {
        ensure!(
            host::command("systemctl", &["is-active", "--quiet", "xray"]).is_err(),
            "Existing Xray is active without node.toml/secrets.json; import validated inputs before takeover to retain client compatibility"
        );
    }
    let backup = store
        .state_dir
        .join("deployment-backups")
        .join(uuid::Uuid::new_v4().to_string());
    store::private_dir(&backup)?;
    let reference = host::Reference {
        binary: PathBuf::from(&c.runtime.xray_binary),
        sha256: String::new(),
        version:
            toml::from_str::<toml::Value>(include_str!("../versions.lock.toml"))?["xray"]["version"]
                .as_str()
                .unwrap()
                .into(),
        probe_url: args.probe_url.clone(),
    };
    let mut journal = Journal {
        schema_version: 1,
        backup: backup.clone(),
        config: c.clone(),
        previous: store.pointer("current")?,
        reference,
        files: vec![],
        dns_zone: args.dns_zone.clone(),
        dns: vec![],
        pending_dns: None,
        routes4: host::default_routes(false)?,
        routes6: host::default_routes(true)?,
        phase: "backed-up".into(),
        config_existed: exists,
        services: host::SERVICES
            .into_iter()
            .map(|name| Ok((name.to_owned(), host::service_state(name)?)))
            .collect::<Result<_>>()?,
    };
    for (i, path) in backup_paths(store, &c).into_iter().enumerate() {
        let copy = backup.join(format!("file-{i}"));
        let existed = fs::symlink_metadata(&path).is_ok();
        if existed {
            copy_tree(&path, &copy)?;
        }
        journal.files.push(Backup {
            fingerprint: if existed {
                fingerprint(&copy)?
            } else {
                Value::Null
            },
            path,
            copy,
            existed,
        });
    }
    save(store, &journal)?;
    #[cfg(feature = "integration-test")]
    if std::env::var("NODECTL_DEPLOY_FAILPOINT").as_deref() == Ok("journal") {
        bail!("Injected deployment journal interruption");
    }
    let result = (|| -> Result<()> {
        let mut created_nginx_default = None;
        if !args.skip_dependencies {
            // Prevent package maintainer scripts from starting/stopping live services.
            store::atomic_write(Path::new("/usr/sbin/policy-rc.d"), b"#!/bin/sh\nexit 101\n")?;
            host::command("chmod", &["0755", "/usr/sbin/policy-rc.d"])?;
            let provision = provision::dependencies(&c, &backup);
            let policy = journal
                .files
                .iter()
                .find(|b| b.path == Path::new("/usr/sbin/policy-rc.d"))
                .unwrap();
            remove(&policy.path)?;
            if policy.existed {
                copy_tree(&policy.copy, &policy.path)?;
            }
            created_nginx_default = provision?;
        }
        if exists {
            store::atomic_write(
                &store.config_dir.join("node.toml"),
                toml::to_string_pretty(&c)?.as_bytes(),
            )?;
        } else {
            initialize(store, &c, args.input.is_some(), &backup)?;
        }
        for change in &changes {
            cf.as_ref()
                .context("Cloudflare client")?
                .ensure_unchanged(change)?;
            journal.pending_dns = Some(change.clone());
            save(store, &journal)?;
            let record = cf.as_ref().context("Cloudflare client")?.apply(change)?;
            journal.dns.push((change.clone(), record));
            journal.pending_dns = None;
            save(store, &journal)?;
        }
        dns::wait(&c, args.dns_wait_seconds)?;
        let token =
            if !args.manual_certificates && host::certificates_part(&c, Some(false)).is_err() {
                Some(
                    cf.as_ref()
                        .context("Automatic DNS-01 requires --dns-zone")?
                        .credential()
                        .to_owned(),
                )
            } else {
                None
            };
        provision::certificates(
            &c,
            &args,
            &backup,
            token.as_deref(),
            superseded_acme.as_deref(),
        )?;
        provision::warp(&c, &args)?;
        journal.phase = "warp-probe".into();
        save(store, &journal)?;
        provision::warp_probe(&c)?;
        let release = store.render()?;
        print!("{}", store.diff(&release)?);
        journal.reference.sha256 = store::file_digest(&journal.reference.binary)?;
        journal.phase = "checking".into();
        save(store, &journal)?;
        host::check(store, &release, journal.reference.clone())?;
        let owned_defaults = entrypoints_absent && created_nginx_default.is_some();
        if owned_defaults {
            for (path, _) in host::ENTRIES {
                if path == "/etc/nginx/nginx.conf" {
                    ensure!(
                        Some(store::file_digest(Path::new(path))?) == created_nginx_default,
                        "New package default changed before apply"
                    );
                } else {
                    ensure!(
                        fs::symlink_metadata(path).is_err(),
                        "Unexpected entrypoint appeared during provisioning; review a new takeover plan"
                    );
                }
            }
        }
        if args.takeover || owned_defaults {
            let record = store.state_dir.join("takeover").join(&release);
            if !record.exists() {
                host::takeover(store, &release)?;
            }
        }
        journal.phase = "applying".into();
        save(store, &journal)?;
        host::apply(store, &release)?;
        if preserve_clients {
            host::previous_clients(
                store,
                previous.as_deref().context("Previous release")?,
                &journal.reference,
            )?;
            println!("Previous client profiles: all three protocol handshakes passed");
        }
        journal.phase = "applied".into();
        save(store, &journal)?;
        if !args.manual_certificates {
            provision::renewal(&c, store, superseded_acme.as_deref())?;
        }
        ensure!(
            journal.routes4 == host::default_routes(false)?
                && journal.routes6 == host::default_routes(true)?,
            "DMIT default routes changed"
        );
        store::atomic_write(
            &backup.join("completed.json"),
            &serde_json::to_vec_pretty(
                &json!({"release":release,"plan_sha256":digest,"dns":journal.dns,"routes_preserved":true}),
            )?,
        )?;
        fs::remove_file(store.state_dir.join("deployment.json"))?;
        println!(
            "Deployment {release} completed; all three protocol handshakes passed. Backup: {}",
            backup.display()
        );
        Ok(())
    })();
    if let Err(error) = result {
        // Service apply has its own transaction. Provisioning uses the saved journal.
        match restore(store, &journal, cf.as_ref()) {
            Ok(()) => bail!(
                "Deployment failed and saved host inputs were restored: {error:#}. Package/CA side effects remain recorded at {}",
                backup.display()
            ),
            Err(restore) => bail!(
                "Deployment failed: {error:#}; recovery also failed: {restore:#}. Retain journal and run recover"
            ),
        }
    }
    Ok(())
}
fn restore(store: &Store, journal: &Journal, cf: Option<&Cloudflare>) -> Result<()> {
    ensure!(
        journal.schema_version == 1
            && journal
                .backup
                .starts_with(store.state_dir.join("deployment-backups")),
        "Invalid deployment recovery record"
    );
    let allowed = backup_paths(store, &journal.config);
    config::validate(&journal.config, &preview_secrets())?;
    for path in &allowed {
        ensure!(
            path.is_absolute()
                && path.components().count() >= 3
                && !store.state_dir.starts_with(path),
            "Unsafe recovery path"
        );
    }
    ensure!(
        journal.services.len() == host::SERVICES.len()
            && host::SERVICES
                .iter()
                .all(|name| journal.services.contains_key(*name)),
        "Incomplete provisioning service state baseline"
    );
    ensure!(
        journal.backup.parent() == Some(store.state_dir.join("deployment-backups").as_path())
            && journal
                .backup
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s)),
        "Invalid recovery directory"
    );
    store::check_private(&journal.backup, true)?;
    let paths = journal
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        journal.files.len() == allowed.len()
            && paths.len() == allowed.len()
            && allowed.iter().all(|p| paths.contains(p)),
        "Incomplete or duplicated provisioning backup list"
    );
    let mut changed_binaries = vec![];
    for file in &journal.files {
        ensure!(
            allowed.contains(&file.path) && file.copy.parent() == Some(journal.backup.as_path()),
            "Invalid recovery backup path"
        );
        if file.existed {
            ensure!(
                fingerprint(&file.copy)? == file.fingerprint,
                "Provisioning backup integrity failure; retain recovery journal"
            );
        }
    }
    // Retain newly generated identities/certificates before undoing preparation,
    // so an operator can reuse a completed issuance/registration after failure.
    let retained = journal.backup.join("failed-material");
    if !retained.exists() {
        let stage = tempfile::Builder::new()
            .prefix(".failed-material-")
            .tempdir_in(&journal.backup)?;
        let mut inventory = BTreeMap::new();
        for (index, file) in journal.files.iter().enumerate() {
            let relevant = file.path == store.config_dir
                || file.path == Path::new(&journal.config.tls.domain_cert_dir)
                || file.path == Path::new(&journal.config.tls.ip_cert_dir)
                || file.path == Path::new(&journal.config.warp.config_file)
                || file.path == Path::new("/root/.acme.sh");
            if relevant
                && fs::symlink_metadata(&file.path).is_ok()
                && fingerprint(&file.path)? != file.fingerprint
            {
                let copy = stage.path().join(format!("file-{index}"));
                copy_tree(&file.path, &copy)?;
                inventory.insert(
                    file.path.display().to_string(),
                    json!({"backup":format!("file-{index}"),"fingerprint":fingerprint(&copy)?}),
                );
            }
        }
        store::atomic_write(
            &stage.path().join("inventory.json"),
            &serde_json::to_vec_pretty(&inventory)?,
        )?;
        fs::rename(stage.path(), retained)?;
    }
    // Restore original binaries/certificates before checking and switching back.
    for file in &journal.files {
        if file.existed && fingerprint(&file.path)? == fingerprint(&file.copy)? {
            continue;
        }
        // A newly installed package remains installed on provisioning failure.
        // Keep its executable consistent with dpkg; restore disabled/inactive state below.
        if !file.existed && file.path == Path::new(&journal.config.runtime.nginx_binary) {
            continue;
        }
        for (name, path) in [
            ("nginx", &journal.config.runtime.nginx_binary),
            ("xray", &journal.config.runtime.xray_binary),
            ("usque-warp", &journal.config.runtime.usque_binary),
        ] {
            if file.path == Path::new(path) && journal.services.get(name).is_some_and(|s| s.active)
            {
                changed_binaries.push(name);
            }
        }
        remove(&file.path)?;
        if file.existed {
            copy_tree(&file.copy, &file.path)?;
        }
    }
    if store.state_dir.join("transaction.json").exists()
        || store.state_dir.join("uninstall.json").exists()
    {
        host::recover(store)?;
    } else {
        for name in changed_binaries {
            host::command("systemctl", &["restart", name])?;
            host::command("systemctl", &["is-active", "--quiet", name])?;
        }
    }
    let current = store.pointer("current")?;
    if current != journal.previous {
        if let Some(previous) = &journal.previous {
            let (c, _) = store.verify(previous)?;
            let mut reference = journal.reference.clone();
            reference.binary = PathBuf::from(c.runtime.xray_binary);
            reference.sha256 = store::file_digest(&reference.binary)?;
            host::check(store, previous, reference)?;
            host::apply(store, previous)?;
        } else if current.is_some() {
            host::uninstall(store, true)?;
        }
    }
    for (name, expected) in &journal.services {
        ensure!(
            ["nginx", "xray", "usque-warp"].contains(&name.as_str()),
            "Unexpected recovery service"
        );
        let active = host::command("systemctl", &["show", name, "-p", "ActiveState", "--value"])?
            .trim()
            == "active";
        if active != expected.active {
            host::command(
                "systemctl",
                &[if expected.active { "start" } else { "stop" }, name],
            )?;
        }
        let actual = host::service_state(name)?;
        if actual.enabled != expected.enabled {
            if actual.enabled == "enabled-runtime" {
                host::command("systemctl", &["disable", "--runtime", name])?;
            } else if actual.enabled == "enabled" {
                host::command("systemctl", &["disable", name])?;
            }
            match expected.enabled.as_str() {
                "enabled" => {
                    host::command("systemctl", &["enable", name])?;
                }
                "enabled-runtime" => {
                    host::command("systemctl", &["enable", "--runtime", name])?;
                }
                _ => {}
            }
        }
        let restored = host::service_state(name)?;
        ensure!(
            restored.active == expected.active && restored.enabled == expected.enabled,
            "Original service state could not be fully restored for {name}"
        );
    }
    if let Some(change) = &journal.pending_dns {
        cf.context("DNS recovery requires Cloudflare token")?
            .restore_pending(change)?;
    }
    for (change, record) in journal.dns.iter().rev() {
        cf.context("DNS recovery requires Cloudflare token")?
            .restore(change, record)?;
    }
    ensure!(
        journal.routes4 == host::default_routes(false)?
            && journal.routes6 == host::default_routes(true)?,
        "Default routes differ; inspect routing before clearing recovery journal"
    );
    fs::remove_file(store.state_dir.join("deployment.json"))?;
    Ok(())
}
pub fn recover(store: &Store) -> Result<()> {
    host::linux_root()?;
    let path = store.state_dir.join("deployment.json");
    store::check_private(&path, false)?;
    let journal: Journal = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| anyhow::anyhow!("Invalid deployment recovery journal"))?;
    let cf = if journal.pending_dns.is_some() || !journal.dns.is_empty() {
        Some(Cloudflare::new(
            journal.dns_zone.as_deref().context("Recovery DNS zone")?,
            dns::token(None)?,
        )?)
    } else {
        None
    };
    restore(store, &journal, cf.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: Deploy,
    }
    fn args(extra: &[&str]) -> Deploy {
        let mut v = vec!["test", "--non-interactive"];
        v.extend(extra);
        Cli::parse_from(v).args
    }
    fn existing() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().join("config"), tmp.path().join("state")).unwrap();
        store::private_dir(&store.config_dir).unwrap();
        store::atomic_write(
            &store.config_dir.join("node.toml"),
            config::EXAMPLE.as_bytes(),
        )
        .unwrap();
        store::atomic_write(
            &store.config_dir.join("secrets.json"),
            &serde_json::to_vec(&preview_secrets()).unwrap(),
        )
        .unwrap();
        (tmp, store)
    }
    #[test]
    fn existing_config_and_client_identities_are_preserved() {
        let (_tmp, store) = existing();
        let before = store.inputs().unwrap();
        let (c, s, exists) = candidate(&store, &args(&[])).unwrap();
        assert!(exists);
        assert_eq!(
            serde_json::to_value(&before.0).unwrap(),
            serde_json::to_value(&c).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&before.1).unwrap(),
            serde_json::to_value(&s).unwrap()
        );
        let (c, s, _) = candidate(
            &store,
            &args(&["--https-port", "4443", "--warp", "false", "--ipv6", ""]),
        )
        .unwrap();
        assert_eq!(c.ports.https, 4443);
        assert!(!c.warp.enabled);
        assert!(c.node.ipv6.is_empty());
        assert_eq!(c.transport.xhttp.path, before.0.transport.xhttp.path);
        assert_eq!(s.reality_private_key, before.1.reality_private_key);
        assert_eq!(
            fs::read_to_string(store.config_dir.join("node.toml")).unwrap(),
            config::EXAMPLE
        );
    }
    #[test]
    fn new_defaults_keep_the_full_dmit_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().join("config"), tmp.path().join("state")).unwrap();
        let (c, _, exists) = candidate(
            &store,
            &args(&[
                "--domain-suffix",
                "example.com",
                "--subdomain-prefix",
                "test-",
                "--ipv4",
                "192.0.2.10",
                "--ipv6",
                "",
            ]),
        )
        .unwrap();
        assert!(!exists);
        assert_eq!(c.node.domain, "test-qwq1.example.com");
        assert_eq!(c.node.reality_domain, "test-reality.example.com");
        assert_eq!(c.node.aliases, vec!["test-www.example.com"]);
        assert_eq!(
            (
                c.clients.xhttp_weight,
                c.clients.ws_weight,
                c.clients.reality_weight
            ),
            (80, 15, 5)
        );
        assert_eq!(
            c.clients.fingerprint_weights,
            toml::from_str::<Config>(config::EXAMPLE)
                .unwrap()
                .clients
                .fingerprint_weights
        );
        assert_eq!(c.tls.protocols, vec!["TLSv1.3"]);
        assert!(c.tls.http2 && c.tls.http3 && c.warp.enabled && !c.warp.take_over_default_route);
        assert_eq!(c.ports.reality, 8443);
        assert!(!store.config_dir.exists() && !store.state_dir.exists());
    }
    #[test]
    fn invalid_existing_inputs_are_never_replaced_with_new_defaults() {
        let (_tmp, store) = existing();
        store::atomic_write(&store.config_dir.join("node.toml"), b"schema_version=999\n").unwrap();
        assert!(candidate(&store, &args(&[])).is_err());
    }
    #[test]
    fn domain_migration_keeps_legacy_snis_and_keys() {
        let (_tmp, store) = existing();
        let (old, secrets) = store.inputs().unwrap();
        let (c, s, _) = candidate(
            &store,
            &args(&[
                "--domain",
                "new.example.net",
                "--reality-domain",
                "reality.example.net",
            ]),
        )
        .unwrap();
        assert!(c.node.aliases.contains(&old.node.domain));
        assert!(c.node.aliases.contains(&old.node.reality_domain));
        assert_eq!(s.reality_private_key, secrets.reality_private_key);
        assert_eq!(c.transport.ws.path, old.transport.ws.path);
        assert!(preserves_clients(&old, &secrets, &c, &s).unwrap());
        let mut changed = c.clone();
        changed.ports.https = 4443;
        assert!(!preserves_clients(&old, &secrets, &changed, &s).unwrap());
    }
    #[test]
    fn provisioning_journal_blocks_all_ordinary_mutations() {
        let (_tmp, store) = existing();
        store::private_dir(&store.state_dir).unwrap();
        store::atomic_write(&store.state_dir.join("deployment.json"), b"{}").unwrap();
        assert!(store.lock().is_err());
        assert!(store.recovery_lock().is_ok());
    }
    #[test]
    fn backup_round_trip_keeps_private_modes_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        store::private_dir(&source).unwrap();
        store::atomic_write(&source.join("key"), b"fixture").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("key", source.join("link")).unwrap();
        let backup = tmp.path().join("backup");
        copy_tree(&source, &backup).unwrap();
        assert_eq!(fingerprint(&source).unwrap(), fingerprint(&backup).unwrap());
        store::check_private(&backup.join("key"), false).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn orphaned_provisioning_child_keeps_recovery_locked() {
        let (_tmp, store) = existing();
        let lock = store.lock().unwrap();
        let lease = provision::Lease::hold(&lock);
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("0.2");
        provision::inherit_lease(&mut command);
        let mut child = command.spawn().unwrap();
        drop(lease);
        drop(lock);
        assert!(store.recovery_lock().is_err());
        assert!(child.wait().unwrap().success());
        assert!(store.recovery_lock().is_ok());
    }
    #[test]
    #[cfg(unix)]
    fn explicit_manual_input_paths_survive_first_deployment_initialization() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().join("config"), tmp.path().join("state")).unwrap();
        let fake = tmp.path().join("xray");
        fs::write(
            &fake,
            "#!/bin/sh\nprintf 'PrivateKey: BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        let mut c: Config = toml::from_str(config::EXAMPLE).unwrap();
        c.runtime.xray_binary = fake.to_str().unwrap().into();
        c.transport.xhttp.path = "/chosen/modules/stream/".into();
        initialize(&store, &c, true, tmp.path()).unwrap();
        let (actual, secrets) = store.inputs().unwrap();
        assert_eq!(actual.transport.xhttp.path, c.transport.xhttp.path);
        assert!(!secrets.xhttp_uuid.is_empty());
    }
}
