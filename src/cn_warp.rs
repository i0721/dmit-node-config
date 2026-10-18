//! CN-only egress. Neither the tunnel nor its marked routing policy owns main.
use crate::{config::Config, host, render, store};
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const RUNTIME_ASSETS: &str = "/var/lib/nodectl-cn-assets";
const ASSET_MARKER: &[u8] = b"nodectl-cn-geodata-v1\n";

fn asset_digests() -> Result<Vec<(&'static str, String)>> {
    let lock: toml::Value = toml::from_str(include_str!("../versions.lock.toml"))?;
    ["geoip", "geosite"]
        .into_iter()
        .map(|name| {
            let digest = lock["geodata"][format!("{name}_sha256")]
                .as_str()
                .context("Missing geodata digest")?;
            Ok((
                if name == "geoip" {
                    "geoip.dat"
                } else {
                    "geosite.dat"
                },
                digest.to_owned(),
            ))
        })
        .collect()
}

pub(crate) fn runtime_asset_dir() -> Result<PathBuf> {
    let digest = store::digest(&serde_json::to_vec(&asset_digests()?)?);
    Ok(Path::new(RUNTIME_ASSETS)
        .join("releases")
        .join(&digest[..32]))
}

pub(crate) fn publish_runtime_assets(store: &store::Store, id: &str) -> Result<()> {
    let (c, _) = store.verify(id)?;
    ensure!(c.routes_cn(), "CN assets require an enabled route");
    let directory = runtime_asset_dir()?;
    crate::web_services::publish_nonsecret_assets(
        Path::new(RUNTIME_ASSETS),
        directory.file_name().unwrap().to_str().unwrap(),
        &store.release(id)?.join("geodata"),
        &["geoip.dat", "geosite.dat"],
        ASSET_MARKER,
    )?;
    runtime_assets_healthy()
}

pub(crate) fn runtime_assets_healthy() -> Result<()> {
    let root = Path::new(RUNTIME_ASSETS);
    store::check_private(&root.join(".nodectl-owned"), false)?;
    ensure!(
        fs::read(root.join(".nodectl-owned"))? == ASSET_MARKER,
        "CN asset ownership record differs"
    );
    let directory = runtime_asset_dir()?;
    for parent in [root.to_path_buf(), root.join("releases"), directory.clone()] {
        let metadata = fs::symlink_metadata(parent)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Unsafe CN asset directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == 0 && metadata.mode() & 0o777 == 0o755,
                "CN asset directories require root ownership and 0755"
            );
        }
    }
    ensure!(
        fs::read_dir(&directory)?.count() == 2,
        "Unexpected CN asset files"
    );
    for (name, digest) in asset_digests()? {
        crate::web_services::check_public_asset(&directory.join(name), &digest)?;
    }
    Ok(())
}

pub fn outbound(c: &Config) -> Value {
    let cn = c.cn_warp.as_ref().expect("validated enabled CN WARP");
    json!({"tag":"cn-warp","protocol":"freedom","streamSettings":{"sockopt":{
        "interface":cn.interface,"mark":cn.socket_mark,"domainStrategy":"ForceIP"}}})
}

pub fn xray(c: &Config, configuration: &mut Value) {
    if !c.routes_cn() {
        return;
    }
    // Route-time resolution alone does not replace Freedom's later OS lookup.
    // Resolve direct sockets through the same tagged DNS model, without marks
    // or device binding, so non-CN queries remain on the native exit.
    if let Some(direct) = configuration["outbounds"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|outbound| outbound["tag"] == "direct")
    {
        direct["streamSettings"] = json!({"sockopt":{"domainStrategy":"ForceIP"}});
    }
    configuration["outbounds"]
        .as_array_mut()
        .unwrap()
        .push(outbound(c));
    // Per-server tags route DNS packets themselves, not only their answers.
    // The CN server is excluded from fallback, and its own fallback is disabled.
    configuration["dns"] = json!({
        "queryStrategy":if c.node.ipv6.is_empty() {"UseIPv4"} else {"UseIP"},
        "disableFallbackIfMatch":true,
        "useSystemHosts":false,
        "servers":[
            {"address":"223.5.5.5","domains":["geosite:cn"],
             "skipFallback":true,"finalQuery":true,"tag":"dns-cn"},
            {"address":"1.1.1.1","tag":"dns-direct"}
        ]
    });
    configuration["routing"] = json!({"domainStrategy":"IPIfNonMatch","rules":[
        {"type":"field","inboundTag":["dns-cn"],"outboundTag":"cn-warp"},
        {"type":"field","inboundTag":["dns-direct"],"outboundTag":"direct"},
        {"type":"field","domain":["geosite:cn"],"outboundTag":"cn-warp"},
        {"type":"field","ip":["geoip:cn"],"outboundTag":"cn-warp"}
    ]});
}

/// Verify the real managed TUN and the very same marked/bound Xray outbound.
/// This does not open a second usque connection or stop the original instance.
pub fn health(c: &Config) -> Result<()> {
    if !c.routes_cn() {
        return Ok(());
    }
    runtime_assets_healthy()?;
    host::command("systemctl", &["is-active", "--quiet", "usque-cn"])?;
    let policy = crate::cn_route::Policy::from(c.cn_warp.as_ref().unwrap());
    ensure!(
        policy.guarded()?,
        "CN WARP routing policy or terminal guard is missing"
    );
    let temporary = tempfile::tempdir()?;
    let socket = TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    let config = temporary.path().join("probe.json");
    store::atomic_write(
        &config,
        &serde_json::to_vec(&json!({
            "log":{"loglevel":"none"},
            "inbounds":[{"listen":"127.0.0.1","port":port,"protocol":"socks","settings":{"auth":"noauth"}}],
            "outbounds":[outbound(c)],
            "dns":{"servers":["1.1.1.1"],"disableFallback":true,"queryStrategy":"UseIPv4"}
        }))?,
    )?;
    drop(socket);
    let mut command = Command::new(&c.runtime.xray_binary);
    command
        .args(["run", "-config", config.to_str().unwrap()])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = std::process::id() as libc::pid_t;
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::other("Probe parent exited"));
                }
                Ok(())
            });
        }
    }
    struct Probe(std::process::Child);
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Probe(command.spawn()?);
    let started = Instant::now();
    loop {
        ensure!(child.0.try_wait()?.is_none(), "CN WARP bound probe exited");
        if let Ok(trace) = host::command(
            "curl",
            &[
                "--disable",
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "8",
                "--noproxy",
                "",
                "--socks5-hostname",
                &format!("127.0.0.1:{port}"),
                "https://www.cloudflare.com/cdn-cgi/trace",
            ],
        ) {
            ensure!(
                trace
                    .lines()
                    .any(|l| ["warp=on", "warp=plus"].contains(&l.trim())),
                "CN interface did not verify a WARP exit"
            );
            return Ok(());
        }
        ensure!(
            started.elapsed() < Duration::from_secs(30),
            "CN WARP interface handshake timed out"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn needs_registration(c: &Config) -> bool {
    (c.warp.enabled && !Path::new(&c.warp.config_file).exists())
        || c.cn_warp
            .as_ref()
            .is_some_and(|cn| cn.enabled && !Path::new(&cn.config_file).exists())
}

pub(crate) fn platform(c: &Config) -> Result<()> {
    if !c.routes_cn() {
        return Ok(());
    }
    let release = host::command("uname", &["-r"])?;
    let parts: Vec<u32> = release
        .trim()
        .split('.')
        .take(2)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        parts.len() == 2 && (parts[0], parts[1]) >= (5, 17),
        "CN routing with minimal CAP_NET_RAW requires Linux 5.17 or later; no capability widening is performed"
    );
    Ok(())
}

pub(crate) fn credentials(c: &Config) -> Result<()> {
    if !c.routes_cn() {
        return Ok(());
    }
    fn read(path: &Path) -> Result<Value> {
        store::check_private(path, false)?;
        ensure!(
            fs::metadata(path)?.len() < 64 * 1024,
            "Invalid independent WARP credential size"
        );
        serde_json::from_slice(&fs::read(path)?)
            .map_err(|_| anyhow::anyhow!("Invalid independent WARP credential record"))
    }
    fn key(value: &Value) -> Result<Vec<u8>> {
        let encoded = value["private_key"]
            .as_str()
            .context("WARP private key is missing")?;
        let normalized = encoded.replace(['\r', '\n'], "");
        let key = base64::engine::general_purpose::STANDARD
            .decode(normalized)
            .map_err(|_| anyhow::anyhow!("Invalid WARP private key encoding"))?;
        ensure!(!key.is_empty(), "WARP private key is empty");
        Ok(key)
    }
    let cn = read(Path::new(&c.cn_warp.as_ref().unwrap().config_file))?;
    let cn_key = key(&cn)?;
    ensure!(
        cn["id"].as_str().is_some_and(|s| !s.is_empty())
            && cn["access_token"].as_str().is_some_and(|s| !s.is_empty()),
        "CN WARP device identity is incomplete"
    );
    if Path::new(&c.warp.config_file).exists() {
        let original = read(Path::new(&c.warp.config_file))?;
        if original["private_key"].is_string() {
            ensure!(
                key(&original)? != cn_key
                    && original["id"] != cn["id"]
                    && original["access_token"] != cn["access_token"],
                "CN WARP must use separately registered credentials; existing instance retained"
            );
        }
    }
    Ok(())
}

/// Copy pinned data into the immutable candidate. Every later verify checks the
/// same source digest as well as the release's own complete file manifest.
pub fn files(
    c: &Config,
    files: &mut BTreeMap<String, Vec<u8>>,
    source: Option<&Path>,
) -> Result<()> {
    let Some(cn) = c.cn_warp.as_ref().filter(|cn| cn.enabled) else {
        return Ok(());
    };
    let lock: toml::Value = toml::from_str(include_str!("../versions.lock.toml"))?;
    for name in ["geoip", "geosite"] {
        let path = source
            .unwrap_or_else(|| Path::new(&cn.geodata_dir))
            .join(format!("{name}.dat"));
        let metadata = fs::symlink_metadata(&path).context("Pinned geodata is not installed")?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 64 * 1024 * 1024,
            "Invalid geodata file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
                "Geodata must be root-owned and not writable by other users"
            );
        }
        let data = fs::read(path)?;
        ensure!(
            Some(store::digest(&data).as_str())
                == lock["geodata"][format!("{name}_sha256")].as_str(),
            "Pinned geodata digest mismatch"
        );
        files.insert(format!("geodata/{name}.dat"), data);
    }
    let vars = BTreeMap::from([
        ("USQUE_BINARY", c.runtime.usque_binary.clone()),
        ("CN_CONFIG", cn.config_file.clone()),
        ("CN_INTERFACE", cn.interface.clone()),
        ("CN_TABLE", cn.routing_table.to_string()),
        ("CN_PRIORITY", cn.rule_priority.to_string()),
        ("CN_MARK", cn.socket_mark.to_string()),
    ]);
    files.insert(
        "systemd/usque-cn.service".into(),
        render::template(include_str!("../templates/usque-cn.service.tpl"), &vars)?.into_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_and_its_dns_have_one_bound_marked_outbound_and_no_fallback() {
        let mut c: Config = toml::from_str(crate::config::EXAMPLE).unwrap();
        c.cn_warp = Some(crate::config::CnWarp {
            enabled: true,
            interface: "warp-cn".into(),
            config_file: "/etc/usque-cn/config.json".into(),
            routing_table: 5271,
            rule_priority: 15271,
            socket_mark: 5271,
            geodata_dir: "/var/lib/nodectl-geodata/202610172205".into(),
        });
        let mut rendered = json!({"outbounds":[{"tag":"direct","protocol":"freedom"}]});
        xray(&c, &mut rendered);
        assert_eq!(rendered["outbounds"][0]["tag"], "direct");
        assert_eq!(
            rendered["outbounds"][0]["streamSettings"]["sockopt"],
            json!({"domainStrategy":"ForceIP"})
        );
        assert_eq!(
            rendered["outbounds"][1]["streamSettings"]["sockopt"],
            json!({"interface":"warp-cn","mark":5271,"domainStrategy":"ForceIP"})
        );
        assert_eq!(rendered["dns"]["disableFallbackIfMatch"], true);
        assert_eq!(rendered["dns"]["servers"][0]["skipFallback"], true);
        assert_eq!(rendered["routing"]["rules"][0]["outboundTag"], "cn-warp");
        assert_eq!(
            rendered["routing"]["rules"][2]["domain"],
            json!(["geosite:cn"])
        );
        assert_eq!(rendered["routing"]["rules"][3]["ip"], json!(["geoip:cn"]));
        c.cn_warp = None;
        let mut original = json!({"outbounds":[]});
        xray(&c, &mut original);
        assert_eq!(original, json!({"outbounds":[]}));
    }
}
