//! Repository lifecycle helpers. No production host changes except explicit download output.
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::{Value, json};
#[path = "../artifacts.rs"]
mod artifacts;
#[path = "../protocol_tools.rs"]
mod protocol_tools;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::Path,
    process::Command,
};

fn command(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("Run {program}"))?;
    ensure!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}
fn arg(args: &[String], index: usize) -> Result<&str> {
    args.get(index)
        .map(String::as_str)
        .context("Missing argument")
}
fn text(path: impl AsRef<Path>) -> Result<String> {
    Ok(fs::read_to_string(path)?
        .trim_start_matches('\u{feff}')
        .to_owned())
}
fn slug(heading: &str) -> String {
    use unicode_general_category::{GeneralCategory::*, get_general_category};
    Regex::new("<[^>]*>")
        .unwrap()
        .replace_all(heading, "")
        .to_lowercase()
        .chars()
        .filter(|c| {
            "-_ ".contains(*c)
                || !matches!(
                    get_general_category(*c),
                    ClosePunctuation
                        | ConnectorPunctuation
                        | DashPunctuation
                        | FinalPunctuation
                        | InitialPunctuation
                        | OpenPunctuation
                        | OtherPunctuation
                        | CurrencySymbol
                        | MathSymbol
                        | ModifierSymbol
                        | OtherSymbol
                        | Control
                        | Format
                        | PrivateUse
                        | Surrogate
                        | Unassigned
                )
        })
        .collect::<String>()
        .replace(' ', "-")
}
fn anchors(path: &Path) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let mut fence = false;
    let heading = Regex::new(r"^#{1,6}\s+(.+?)\s*#*$")?;
    for line in text(path)?.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fence = !fence;
        }
        if fence {
            continue;
        }
        if let Some(c) = heading.captures(line) {
            let name = slug(&c[1]);
            let count = counts.entry(name.clone()).or_insert(0);
            found.insert(if *count == 0 {
                name
            } else {
                format!("{name}-{count}")
            });
            *count += 1;
        }
    }
    Ok(found)
}
fn docs() -> Result<()> {
    let root = fs::canonicalize(".")?;
    let files = command(
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let fence = Regex::new(r"(?s)```.*?```")?;
    let links = Regex::new(r"!?\[[^\]]*\]\((<?[^)\n]+>?)\)")?;
    let remote = Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:")?;
    // The audit must not itself publish an operator's private identifiers.
    let private =
        Regex::new(r"dash\.cloudflare\.com/[a-f0-9]{32}|(?i:[a-z]:[\\/](?:Users|coder)[\\/])")?;
    let mut errors = vec![];
    for name in files.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name = std::str::from_utf8(name)?;
        let path = root.join(name);
        if path.extension().is_none_or(|e| e != "md") || !path.is_file() {
            continue;
        }
        let original = text(&path)?;
        let content = fence.replace_all(&original, "");
        for c in links.captures_iter(&content) {
            let link = c[1].trim().trim_matches(['<', '>']);
            if remote.is_match(link) || link.starts_with("//") {
                continue;
            }
            let (file, fragment) = link.split_once('#').unwrap_or((link, ""));
            let file = file.split('?').next().unwrap();
            let decoded = percent_encoding::percent_decode_str(file).decode_utf8()?;
            let target = if file.is_empty() {
                path.clone()
            } else {
                path.parent().unwrap().join(decoded.as_ref())
            };
            match fs::canonicalize(&target) {
                Ok(target) if target.starts_with(&root) => {
                    if !fragment.is_empty()
                        && target.extension().is_some_and(|e| e == "md")
                        && !anchors(&target)?.contains(
                            percent_encoding::percent_decode_str(fragment)
                                .decode_utf8()?
                                .as_ref(),
                        )
                    {
                        errors.push(format!("{name}: missing anchor {link}"));
                    }
                }
                _ => errors.push(format!("{name}: missing target {link}")),
            }
        }
        if private.is_match(&original) {
            errors.push(format!("{name}: real deployment identifier"));
        }
    }
    for error in &errors {
        eprintln!("{error}");
    }
    ensure!(errors.is_empty(), "Markdown: {} errors", errors.len());
    println!("Markdown targets, anchors and identifiers passed");
    Ok(())
}
fn workflows() -> Result<()> {
    let uses = Regex::new(r"uses:\s*([^\s#]+)")?;
    let immutable = Regex::new(r"^[^@]+@[0-9a-f]{40}$")?;
    for file in fs::read_dir(".github/workflows")? {
        let path = file?.path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        for c in uses.captures_iter(&text(&path)?) {
            ensure!(
                c[1].starts_with("./") || immutable.is_match(&c[1]),
                "{}: mutable action {}",
                path.display(),
                &c[1]
            );
        }
    }
    Ok(())
}
fn native_audit() -> Result<()> {
    let files = command(
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let runtime =
        Regex::new(r"(?i)\bpython(?:[0-9.]*)?\b|\bpyo3\b|\bvirtualenv\b|\bpip[0-9]*\s+install")?;
    for file in files.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let file = std::str::from_utf8(file)?;
        let path = Path::new(file);
        if !path.is_file() {
            continue;
        }
        ensure!(
            ![Some("py"), Some("pyc")].contains(&path.extension().and_then(|v| v.to_str())),
            "Project-owned Python source remains: {file}"
        );
        if file == "src/bin/repo-tools.rs" {
            continue;
        }
        if [
            Some("rs"),
            Some("sh"),
            Some("yml"),
            Some("yaml"),
            Some("toml"),
        ]
        .contains(&path.extension().and_then(|v| v.to_str()))
            || file.contains("Dockerfile")
        {
            ensure!(
                !runtime.is_match(&text(path)?),
                "Project-owned Python dependency remains: {file}"
            );
        }
    }
    println!("Native lifecycle audit passed");
    Ok(())
}
fn visit(
    id: &str,
    nodes: &BTreeMap<String, Value>,
    reachable: &mut BTreeSet<String>,
) -> Result<()> {
    if !reachable.insert(id.into()) {
        return Ok(());
    }
    for dep in nodes.get(id).context("Missing dependency node")?["deps"]
        .as_array()
        .context("Missing deps")?
    {
        if dep["dep_kinds"]
            .as_array()
            .context("Missing dep kinds")?
            .iter()
            .any(|v| v["kind"] != "dev")
        {
            visit(
                dep["pkg"].as_str().context("Dependency id")?,
                nodes,
                reachable,
            )?;
        }
    }
    Ok(())
}
fn licenses(work: &Path) -> Result<()> {
    let metadata: Value = serde_json::from_slice(&fs::read(work.join("cargo-metadata.json"))?)?;
    let destination = work.join("licenses");
    fs::create_dir(&destination)?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .context("Metadata nodes")?
        .iter()
        .map(|v| Ok((v["id"].as_str().context("Node id")?.to_owned(), v.clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut reachable = BTreeSet::new();
    visit(
        metadata["resolve"]["root"]
            .as_str()
            .context("Metadata root")?,
        &nodes,
        &mut reachable,
    )?;
    let mut inventory = vec![];
    for package in metadata["packages"].as_array().context("Packages")? {
        if !reachable.contains(package["id"].as_str().context("Package id")?) {
            continue;
        }
        let name = package["name"].as_str().context("Package name")?;
        let version = package["version"].as_str().context("Package version")?;
        let manifest = Path::new(package["manifest_path"].as_str().context("Manifest")?);
        let mut notices = vec![];
        for file in fs::read_dir(manifest.parent().context("Package dir")?)? {
            let path = file?.path();
            let file = path
                .file_name()
                .context("Notice name")?
                .to_string_lossy()
                .to_string();
            if path.is_file()
                && ["license", "licence", "copying", "notice", "authors"]
                    .iter()
                    .any(|prefix| file.to_lowercase().starts_with(prefix))
            {
                let dir = destination.join(format!("{name}-{version}"));
                fs::create_dir_all(&dir)?;
                fs::copy(path, dir.join(&file))?;
                notices.push(file);
            }
        }
        ensure!(
            !notices.is_empty(),
            "Missing upstream license notices for {name}-{version}"
        );
        notices.sort();
        inventory.push(json!({"name":name,"version":version,"license":package["license"],"repository":package["repository"],"notices":notices}));
    }
    fs::write(
        destination.join("inventory.json"),
        format!("{}\n", serde_json::to_string_pretty(&inventory)?),
    )?;
    fs::remove_file(work.join("cargo-metadata.json"))?;
    Ok(())
}
fn smoke(binary: &Path, work: &Path) -> Result<()> {
    let config = work.join("config");
    fs::create_dir(&config)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700))?;
    }
    let mut legacy: toml::Value = toml::from_str(include_str!("../../config/node.example.toml"))?;
    legacy.as_table_mut().unwrap().remove("vless");
    legacy.as_table_mut().unwrap().remove("distribution");
    fs::write(config.join("node.toml"), toml::to_string_pretty(&legacy)?)?;
    fs::write(
        config.join("secrets.json"),
        serde_json::to_vec(
            &json!({"xhttp_uuid":"10a6cfe1-d381-4c46-865e-998b28b801a1","ws_uuid":"20a6cfe1-d381-4c46-865e-998b28b801a2","reality_uuid":"30a6cfe1-d381-4c46-865e-998b28b801a3","reality_private_key":"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc","reality_short_id":"0123456789abcdef"}),
        )?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["node.toml", "secrets.json"] {
            fs::set_permissions(config.join(name), fs::Permissions::from_mode(0o600))?;
        }
    }
    ensure!(
        Command::new(binary)
            .arg("--config-dir")
            .arg(config)
            .arg("--state-dir")
            .arg(work.join("state"))
            .arg("validate")
            .status()?
            .success(),
        "Release compatibility smoke failed"
    );
    Ok(())
}
fn probe() -> Result<()> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    use std::{net::TcpListener, sync::Arc};
    let certs = CertificateDer::pem_file_iter("/etc/ssl/dmit/domain/fullchain.pem")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_file("/etc/ssl/dmit/domain/privkey.pem")?;
    let config = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?,
    );
    for socket in TcpListener::bind("127.0.0.1:9443")?.incoming() {
        let mut socket = socket?;
        socket.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        socket.set_write_timeout(Some(std::time::Duration::from_secs(10)))?;
        let config = config.clone();
        std::thread::spawn(move || -> Result<()> {
            let mut conn = rustls::ServerConnection::new(config)?;
            let mut tls = rustls::Stream::new(&mut conn, &mut socket);
            let mut request = vec![];
            let mut byte = [0];
            while request.len() < 16384 && !request.ends_with(b"\r\n\r\n") {
                ensure!(tls.read(&mut byte)? != 0, "Incomplete request");
                request.push(byte[0]);
            }
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 18\r\nConnection: close\r\n\r\nintegration-ready\n")?;
            tls.flush()?;
            Ok(())
        });
    }
    Ok(())
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match arg(&args, 0)? {
        "version" => println!("{}", env!("CARGO_PKG_VERSION")),
        "check-docs" => docs()?,
        "check-workflows" => workflows()?,
        "check-native" => native_audit()?,
        "download-xray" => artifacts::download_xray(Path::new(arg(&args, 1)?))?,
        "download-mihomo" => artifacts::download_mihomo(Path::new(arg(&args, 1)?))?,
        "release-licenses" => licenses(Path::new(arg(&args, 1)?))?,
        "release-smoke" => {
            let binary = fs::canonicalize(arg(&args, 1)?)?;
            if let Some(work) = args.get(2) {
                smoke(&binary, Path::new(work))?;
            } else {
                let work = tempfile::tempdir()?;
                smoke(&binary, work.path())?;
            }
        }
        "ipv4" => {
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            let v: Value = serde_json::from_str(&input)?;
            let ip = v[0]["addr_info"]
                .as_array()
                .context("Address list")?
                .iter()
                .find(|v| v["family"] == "inet")
                .context("No IPv4")?["local"]
                .as_str()
                .context("IPv4 local")?;
            let _: std::net::Ipv4Addr = ip.parse()?;
            println!("{ip}");
        }
        "probe" => probe()?,
        "protocol-config" => protocol_tools::configs(
            Path::new(arg(&args, 1)?),
            arg(&args, 2)?,
            arg(&args, 3)?,
            arg(&args, 4)?,
            Path::new(arg(&args, 5)?),
            args.get(6).map(String::as_str),
        )?,
        "json-digest" => {
            use sha2::{Digest, Sha256};
            let value: Value = serde_json::from_slice(&fs::read(arg(&args, 1)?)?)?;
            println!(
                "{}",
                hex::encode(Sha256::digest(serde_json::to_vec(&value)?))
            );
        }
        "udp-echo" => protocol_tools::echo(arg(&args, 1)?, false)?,
        "hello-relay" => protocol_tools::hello_relay(Path::new(arg(&args, 1)?))?,
        "legacy-inputs-fixture" => protocol_tools::legacy_inputs(Path::new(arg(&args, 1)?))?,
        "hello-check" => protocol_tools::hello_check(Path::new(arg(&args, 1)?), arg(&args, 2)?)?,
        "udp-dns" => protocol_tools::echo(arg(&args, 1)?, true)?,
        "udp-probe" => protocol_tools::udp(
            arg(&args, 1)?.parse()?,
            arg(&args, 2)?.parse()?,
            arg(&args, 3)?.parse()?,
            args.get(4).map(String::as_str).unwrap_or("echo"),
        )?,
        _ => bail!("Unknown repository helper"),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("repo-tools: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_heading_slug() {
        assert_eq!(slug("3. Cloudflare API Token"), "3-cloudflare-api-token");
        assert_eq!(slug("图 2：可选 IPv6 AAAA 记录"), "图-2可选-ipv6-aaaa-记录");
    }
    #[test]
    fn repeated_and_fenced_headings() {
        let f = tempfile::NamedTempFile::new().unwrap();
        fs::write(f.path(), "# A\n# A\n```\n# B\n```\n").unwrap();
        assert_eq!(
            anchors(f.path()).unwrap(),
            BTreeSet::from(["a".into(), "a-1".into()])
        );
    }
}
