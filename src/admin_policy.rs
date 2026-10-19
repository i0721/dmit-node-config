//! The Rust authority for Web changes. Unknown/new fields are protected by default.
use crate::{
    config::{self, Config},
    store::{self, Store},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;

pub const ASSETS: &[&str] = &[
    "node.domain",
    "node.reality_domain",
    "node.aliases",
    "node.ipv4",
    "node.ipv6",
];
pub fn changes(a: &Value, b: &Value, prefix: &str, result: &mut Vec<Value>) {
    if a == b {
        return;
    }
    if let (Some(a), Some(b)) = (a.as_object(), b.as_object()) {
        let mut keys: Vec<_> = a.keys().chain(b.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let p = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            changes(
                a.get(key).unwrap_or(&Value::Null),
                b.get(key).unwrap_or(&Value::Null),
                &p,
                result,
            );
        }
    } else {
        result.push(
            json!({"field":prefix,"before":a,"after":b,"protected":!ASSETS.contains(&prefix)}),
        );
    }
}
pub fn enforce(old: &Config, new: &Config, advanced: bool) -> Result<Vec<Value>> {
    let mut diff = vec![];
    changes(
        &serde_json::to_value(old)?,
        &serde_json::to_value(new)?,
        "",
        &mut diff,
    );
    ensure!(
        advanced || !diff.iter().any(|d| d["protected"] == true),
        "protected_field"
    );
    // These paths would permit root execution or arbitrary privileged file reads.
    let a = serde_json::to_value(old)?;
    let b = serde_json::to_value(new)?;
    for key in ["schema_version", "runtime", "website"] {
        ensure!(a[key] == b[key], "immutable_platform_boundary");
    }
    if let Some(cn) = &new.cn_warp {
        let baseline = old
            .cn_warp
            .clone()
            .unwrap_or_else(|| config::CnWarp::managed(false));
        ensure!(
            cn.config_file == baseline.config_file && cn.geodata_dir == baseline.geodata_dir,
            "immutable_platform_boundary"
        );
    }
    if let Some(web) = &new.web_services {
        let baseline = old
            .web_services
            .clone()
            .unwrap_or_else(|| config::WebServices::managed(false));
        ensure!(
            web.openlist_data_dir == baseline.openlist_data_dir
                && web.openlist_files_dir == baseline.openlist_files_dir,
            "immutable_platform_boundary"
        );
        ensure!(
            ![web.librespeed_port, web.openlist_port]
                .iter()
                .any(|p| [22, 9080, 9443].contains(p)),
            "reserved_port"
        );
    }
    ensure!(
        new.tls.domain_cert_dir == old.tls.domain_cert_dir
            && new.tls.ip_cert_dir == old.tls.ip_cert_dir
            && new.warp.config_file == old.warp.config_file
            && new.transport.xhttp.socket == old.transport.xhttp.socket
            && new.transport.ws.socket == old.transport.ws.socket,
        "immutable_platform_boundary"
    );
    ensure!(
        !new.warp.take_over_default_route
            && new.tls.protocols == ["TLSv1.3"]
            && new.tls.http2
            && new.tls.http3,
        "dmit_invariant"
    );
    ensure!(
        ![new.ports.http, new.ports.https, new.ports.reality]
            .iter()
            .any(|p| [22, 9080, 9443].contains(p)),
        "reserved_port"
    );
    Ok(diff)
}
pub fn fingerprint(s: &Store) -> Result<String> {
    let n = fs::read(s.config_dir.join("node.toml"))?;
    let secret = fs::read(s.config_dir.join("secrets.json"))?;
    Ok(store::digest(&serde_json::to_vec(&(
        store::digest(&n),
        store::digest(&secret),
        s.pointer("current")?,
    ))?))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputJournal {
    pub before: String,
    pub candidate: String,
    pub previous: Option<String>,
    #[serde(default)]
    pub before_secrets: Option<String>,
}
pub fn recover_inputs(s: &Store) -> Result<()> {
    let p = s.state_dir.join("admin-input.json");
    if !p.exists() {
        return Ok(());
    }
    store::check_private(&p, false)?;
    store::check_private(&s.config_dir, true)?;
    store::check_private(&s.config_dir.join("node.toml"), false)?;
    ensure!(
        !s.state_dir.join("transaction.json").exists()
            && !s.state_dir.join("deployment.json").exists(),
        "Recover host transaction first"
    );
    let j: InputJournal = serde_json::from_slice(&fs::read(&p)?)?;
    let current = s.pointer("current")?;
    let committed = current.as_deref() == Some(&j.candidate);
    let secret_restore = if let Some(before) = &j.before_secrets {
        Some(if committed {
            fs::read(s.release(&j.candidate)?.join("source/secrets.json"))?
        } else {
            before.as_bytes().to_vec()
        })
    } else {
        None
    };
    let bytes = if committed {
        fs::read(s.release(&j.candidate)?.join("source/node.toml"))?
    } else {
        ensure!(current == j.previous, "Input recovery pointer mismatch");
        j.before.into_bytes()
    };
    // Check candidate against the retained secret before committing input recovery.
    let c: Config = toml::from_str(std::str::from_utf8(&bytes)?)?;
    store::check_private(&s.config_dir.join("secrets.json"), false)?;
    let secrets: config::Secrets = serde_json::from_slice(
        &secret_restore
            .clone()
            .unwrap_or(fs::read(s.config_dir.join("secrets.json"))?),
    )?;
    config::validate(&c, &secrets)?;
    if let Some(bytes) = secret_restore {
        store::atomic_write(&s.config_dir.join("secrets.json"), &bytes)?;
    }
    store::atomic_write(&s.config_dir.join("node.toml"), &bytes)?;
    fs::remove_file(p)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_mode_is_default_and_advanced_has_hard_limits() {
        let c: Config = toml::from_str(config::EXAMPLE).unwrap();
        let mut n = c.clone();
        assert!(enforce(&c, &n, false).unwrap().is_empty());
        n.node.domain = "speed.test.org".into();
        assert!(enforce(&c, &n, false).is_ok());
        n.clients.ws_weight += 1;
        assert!(enforce(&c, &n, false).is_err());
        assert!(enforce(&c, &n, true).is_ok());
        n.warp.take_over_default_route = true;
        assert!(enforce(&c, &n, true).is_err());
        n = c.clone();
        n.tls.http3 = false;
        assert!(enforce(&c, &n, true).is_err());
        n = c.clone();
        n.runtime.nginx_binary = "/tmp/evil".into();
        assert!(enforce(&c, &n, true).is_err());
        n = c.clone();
        n.ports.reality = 22;
        assert!(enforce(&c, &n, true).is_err());
        n = c.clone();
        n.transport.xhttp.socket = "/etc/console-escape.sock".into();
        assert!(enforce(&c, &n, true).is_err());
        n = c.clone();
        n.transport.ws.socket = "/var/log/console-escape.sock".into();
        assert!(enforce(&c, &n, true).is_err());
    }
}
