use crate::{host, store::Store};
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::json;
use std::{fs, path::Path};

#[derive(Serialize)]
pub struct Finding {
    name: String,
    ok: bool,
    detail: String,
    remedy: String,
}

fn add(items: &mut Vec<Finding>, name: &str, result: Result<()>, remedy: &str) {
    items.push(Finding {
        name: name.into(),
        ok: result.is_ok(),
        // Never expose external output, parser input or credentials.
        detail: if result.is_ok() {
            "passed"
        } else {
            "needs attention"
        }
        .into(),
        remedy: remedy.into(),
    });
}

fn verify_listeners(text: &str, managed: bool, expected: &Path) -> Result<()> {
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        anyhow::ensure!(managed, "Unmanaged listener");
        let mut found = false;
        for part in line.split("pid=").skip(1) {
            let pid = part
                .split(',')
                .next()
                .ok_or_else(|| anyhow::anyhow!("Missing listener PID"))?
                .parse::<u32>()?;
            anyhow::ensure!(
                fs::read_link(format!("/proc/{pid}/exe"))? == expected,
                "Listener executable mismatch"
            );
            found = true;
        }
        anyhow::ensure!(found, "Cannot identify listener process");
    }
    Ok(())
}

pub fn doctor(store: &Store, as_json: bool) -> Result<()> {
    let mut items = vec![];
    add(
        &mut items,
        "linux-systemd",
        (|| {
            anyhow::ensure!(
                cfg!(target_os = "linux") && Path::new("/run/systemd/system").is_dir(),
                "systemd unavailable"
            );
            host::command("systemctl", &["show", "--property=Version", "--value"])?;
            Ok(())
        })(),
        "Use Debian 12/13 or Ubuntu 24.04/26.04 booted with systemd.",
    );
    for (tool, args, remedy) in [
        (
            "openssl",
            vec!["version"],
            "Install openssl and ca-certificates.",
        ),
        (
            "curl",
            vec!["--version"],
            "Install curl; HTTP3 is required for the external QUIC test.",
        ),
        ("ip", vec!["-Version"], "Install iproute2."),
        ("runuser", vec!["--version"], "Install util-linux."),
        ("systemd-analyze", vec!["--version"], "Install systemd."),
    ] {
        add(
            &mut items,
            tool,
            host::command(tool, &args).map(|_| ()),
            remedy,
        );
    }
    match store.inputs() {
        Err(_) => add(
            &mut items,
            "configuration",
            Err(anyhow::anyhow!("not ready")),
            "Run nodectl setup, or fix node.toml/secrets.json and their owner-only permissions; then validate.",
        ),
        Ok((c, _)) => {
            add(
                &mut items,
                "configuration",
                Ok(()),
                "Run validate after editing inputs.",
            );
            add(
                &mut items,
                "xray-version",
                (|| {
                    let text = host::command(&c.runtime.xray_binary, &["version"])?;
                    let lock: toml::Value = toml::from_str(include_str!("../versions.lock.toml"))?;
                    anyhow::ensure!(
                        text.split_whitespace()
                            .any(|v| Some(v) == lock["xray"]["version"].as_str()),
                        "version mismatch"
                    );
                    Ok(())
                })(),
                "Install the trusted Xray version pinned in versions.lock.toml.",
            );
            add(
                &mut items,
                "nginx-h2-h3",
                (|| {
                    // nginx -V writes build flags to stderr. Capture it without exposing it.
                    let text = host::command_stderr(&c.runtime.nginx_binary, &["-V"])?;
                    let version = text
                        .split_whitespace()
                        .find_map(|v| v.strip_prefix("nginx/"))
                        .ok_or_else(|| anyhow::anyhow!("Unknown nginx version"))?;
                    let version = version
                        .split('.')
                        .map(str::parse::<u32>)
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    anyhow::ensure!(
                        version.as_slice() >= [1, 25, 1].as_slice(),
                        "Unsupported nginx version"
                    );
                    anyhow::ensure!(
                        text.contains("--with-http_ssl_module")
                            && (!c.tls.http2 || text.contains("--with-http_v2_module"))
                            && (!c.tls.http3 || text.contains("--with-http_v3_module")),
                        "missing modules"
                    );
                    Ok(())
                })(),
                "Install official Nginx >=1.25.1 with SSL, HTTP2 and HTTP3 modules.",
            );
            add(
                &mut items,
                "certificates",
                host::certificates(&c),
                "Provision both CA-trusted SAN certificates; domain ECDSA P-384, keys owner-only, >=24 hours remaining.",
            );
            for user in [&c.runtime.xray_user, &c.runtime.nginx_user] {
                add(
                    &mut items,
                    &format!("user:{user}"),
                    host::command("id", &[user]).map(|_| ()),
                    "Create the configured system users and the Nginx group; see installation.md.",
                );
            }
            for (user, flag, dir) in [
                (&c.runtime.nginx_user, "-x", c.website.root.clone()),
                (
                    &c.runtime.xray_user,
                    "-w",
                    format!("{}/xray", c.runtime.log_root),
                ),
            ] {
                add(
                    &mut items,
                    &format!("directory:{dir}"),
                    host::command(
                        "runuser",
                        &[
                            "-u",
                            user,
                            "-g",
                            &c.runtime.nginx_user,
                            "--",
                            "test",
                            flag,
                            &dir,
                        ],
                    )
                    .map(|_| ()),
                    "Prepare service-owned log directories and readable/searchable website directories.",
                );
            }
            let managed = store.pointer("current").ok().flatten().is_some();
            for port in [c.ports.http, c.ports.https, c.ports.reality] {
                add(
                    &mut items,
                    &format!("tcp-port:{port}"),
                    (|| {
                        let text = host::command(
                            "ss",
                            &["-H", "-ltnp", "sport", "=", &format!(":{port}")],
                        )?;
                        let expected = if port == c.ports.reality {
                            &c.runtime.xray_binary
                        } else {
                            &c.runtime.nginx_binary
                        };
                        verify_listeners(&text, managed, Path::new(expected))?;
                        Ok(())
                    })(),
                    "Run as root to identify listeners; release conflicting processes or repair the managed service executable.",
                );
            }
            if c.tls.http3 {
                add(
                    &mut items,
                    "udp-quic-port",
                    (|| {
                        let text = host::command(
                            "ss",
                            &["-H", "-lunp", "sport", "=", &format!(":{}", c.ports.https)],
                        )?;
                        verify_listeners(&text, managed, Path::new(&c.runtime.nginx_binary))
                    })(),
                    "Release conflicting UDP listeners; after apply verify UDP 443 from another network with an HTTP3 client.",
                );
            }
            for socket in [&c.transport.xhttp.socket, &c.transport.ws.socket] {
                add(
                    &mut items,
                    &format!("uds:{socket}"),
                    (|| {
                        let parent = Path::new(socket)
                            .parent()
                            .ok_or_else(|| anyhow::anyhow!("Missing socket parent"))?
                            .to_str()
                            .ok_or_else(|| anyhow::anyhow!("Non UTF8 path"))?;
                        host::command(
                            "runuser",
                            &[
                                "-u",
                                &c.runtime.xray_user,
                                "-g",
                                &c.runtime.nginx_user,
                                "--",
                                "test",
                                "-w",
                                parent,
                            ],
                        )?;
                        host::command(
                            "runuser",
                            &["-u", &c.runtime.nginx_user, "--", "test", "-x", parent],
                        )?;
                        if managed {
                            host::command("test", &["-S", socket])?;
                            host::command(
                                "runuser",
                                &["-u", &c.runtime.nginx_user, "--", "test", "-w", socket],
                            )?;
                        } else {
                            anyhow::ensure!(
                                !Path::new(socket).exists(),
                                "Unmanaged socket collision"
                            );
                        }
                        Ok(())
                    })(),
                    "Prepare a writable/searchable parent; deployed UDS must be xray:www-data 0660 and accessible to Nginx.",
                );
            }
            if c.warp.enabled {
                add(
                    &mut items,
                    "warp",
                    (|| {
                        crate::store::check_private(Path::new(&c.warp.config_file), false)?;
                        let _: serde_json::Value =
                            serde_json::from_slice(&fs::read(&c.warp.config_file)?)?;
                        anyhow::ensure!(Path::new("/dev/net/tun").exists(), "missing TUN");
                        host::command(&c.runtime.usque_binary, &["--help"])?;
                        Ok(())
                    })(),
                    "Register usque once, protect its JSON as 0600, and enable /dev/net/tun; do not change the default route.",
                );
            }
        }
    }
    let healthy = items.iter().all(|f| f.ok);
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"ok":healthy,"checks":items}))?
        );
    } else {
        for finding in items {
            println!(
                "{} {}: {}",
                if finding.ok { "OK" } else { "FAIL" },
                finding.name,
                finding.detail
            );
            if !finding.ok {
                println!("  Fix: {}", finding.remedy);
            }
        }
    }
    if !healthy {
        bail!("Doctor found issues; address failed checks, then rerun doctor.");
    }
    Ok(())
}

pub fn status(store: &Store, as_json: bool) -> Result<()> {
    let mut services = std::collections::BTreeMap::new();
    for name in ["nginx", "xray", "usque-warp"] {
        let state = host::command("systemctl", &["show", name, "-p", "ActiveState", "--value"])
            .map(|v| v.trim().to_owned())
            .unwrap_or_else(|_| "unavailable".into());
        services.insert(name, state);
    }
    let current = store.pointer("current");
    let current_valid = current.is_ok();
    let current = current.ok().flatten();
    let previous = store.pointer("previous").ok().flatten();
    let checked = current
        .as_ref()
        .is_some_and(|id| store.state_dir.join("checks").join(id).is_file());
    let checked_at = current
        .as_ref()
        .and_then(|id| fs::metadata(store.state_dir.join("checks").join(id)).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let certs = store
        .inputs()
        .map(|(c, _)| host::certificates(&c).is_ok())
        .unwrap_or(false);
    let report = json!({"tool_version":env!("CARGO_PKG_VERSION"), "current":current,"current_integrity":current_valid,
        "previous":previous,"pending_transaction":store.state_dir.join("transaction.json").exists(),
        "pending_uninstall":store.state_dir.join("uninstall.json").exists(),
        "check_receipt_present":checked,"last_check_unix_seconds":checked_at,"certificates_valid_24h":certs,"services":services});
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "nodectl {}\nCurrent: {}\nPrevious: {}\nCurrent integrity: {}\nCheck receipt: {}\nCertificates >=24h: {}\nPending transaction: {}\nPending uninstall: {}",
            env!("CARGO_PKG_VERSION"),
            current.as_deref().unwrap_or("none"),
            previous.as_deref().unwrap_or("none"),
            current_valid,
            checked,
            certs,
            report["pending_transaction"],
            report["pending_uninstall"]
        );
        for (name, state) in services {
            println!("{name}: {state}");
        }
        println!(
            "Last check (Unix UTC seconds): {}",
            checked_at
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".into())
        );
    }
    Ok(())
}

pub fn migrate(store: &Store) -> Result<()> {
    let (c, _) = store.inputs()?;
    anyhow::ensure!(
        c.schema_version == 1,
        "Unsupported schema; restore inputs and use their original tool"
    );
    println!("Schema 1 is current; validated inputs preserved byte-for-byte. No migration needed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn occupied_ports_require_identified_managed_executable() {
        assert!(
            super::verify_listeners("LISTEN 0 128 0:443", false, std::path::Path::new("/nginx"))
                .is_err()
        );
        assert!(
            super::verify_listeners("LISTEN 0 128 0:443", true, std::path::Path::new("/nginx"))
                .is_err()
        );
        assert!(super::verify_listeners("", false, std::path::Path::new("/nginx")).is_ok());
    }
    #[test]
    fn findings_never_contain_external_error_details() {
        let mut findings = vec![];
        super::add(
            &mut findings,
            "tool",
            Err(anyhow::anyhow!("SECRET-CREDENTIAL")),
            "install tool",
        );
        let result = serde_json::to_string(&findings).unwrap();
        assert!(!result.contains("SECRET"));
        assert!(result.contains("install tool"));
    }
}
