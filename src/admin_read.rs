//! Read-only observations; unavailable is null/error, never invented zero metrics.
use crate::{config::Config, host, share, store::Store};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, time::Instant};
pub const SERVICES: &[&str] = &[
    "nginx",
    "xray",
    "usque-warp",
    "usque-cn",
    "nodectl-share",
    "librespeed",
    "openlist",
];
type CounterSnapshot = (Instant, u64, u64, BTreeMap<String, (u64, u64)>);
#[derive(Default)]
pub struct Meter {
    previous: Option<CounterSnapshot>,
}
fn cpu(text: &str) -> Option<(u64, u64)> {
    let values = text
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    Some((
        values.iter().sum(),
        values.get(3)? + values.get(4).unwrap_or(&0),
    ))
}
fn net(text: &str) -> BTreeMap<String, (u64, u64)> {
    text.lines()
        .filter_map(|line| {
            let (name, values) = line.split_once(':')?;
            let v: Vec<_> = values.split_whitespace().collect();
            Some((
                name.trim().into(),
                (v.first()?.parse().ok()?, v.get(8)?.parse().ok()?),
            ))
        })
        .collect()
}
impl Meter {
    pub fn sample(&mut self) -> Value {
        let observed = Instant::now();
        let c = fs::read_to_string("/proc/stat").ok().and_then(|s| cpu(&s));
        let interfaces = fs::read_to_string("/proc/net/dev").ok().map(|s| net(&s));
        let cpu_percent = c.and_then(|(total, idle)| {
            self.previous.as_ref().and_then(|(_, t, i, _)| {
                let delta = total.checked_sub(*t)?;
                if delta == 0 {
                    None
                } else {
                    Some(100.0 * (delta - idle.saturating_sub(*i).min(delta)) as f64 / delta as f64)
                }
            })
        });
        let network=interfaces.as_ref().map(|interfaces|interfaces.iter().map(|(name,(rx,tx))| {
            let rate=self.previous.as_ref().and_then(|(time,_,_,old)|{let(r,t)=old.get(name)?;let seconds=observed.duration_since(*time).as_secs_f64();(seconds>=0.1).then_some((rx.checked_sub(*r)? as f64/seconds,tx.checked_sub(*t)? as f64/seconds))});
            json!({"interface":name,"rx_bytes":rx,"tx_bytes":tx,"rx_bytes_per_second":rate.map(|v|v.0),"tx_bytes_per_second":rate.map(|v|v.1)})
        }).collect::<Vec<_>>());
        if let (Some((total, idle)), Some(interfaces)) = (c, interfaces) {
            self.previous = Some((observed, total, idle, interfaces));
        }
        let memory=fs::read_to_string("/proc/meminfo").ok().and_then(|s|{
            let mut fields=BTreeMap::new();for line in s.lines(){let(k,v)=line.split_once(':')?;fields.insert(k,v.split_whitespace().next()?.parse::<u64>().ok()?*1024);}
            let total=*fields.get("MemTotal")?;let available=*fields.get("MemAvailable")?;Some(json!({"total_bytes":total,"available_bytes":available,"used_bytes":total.saturating_sub(available)}))
        });
        json!({"sample_unix":crate::admin_auth::now(),"cpu_percent":cpu_percent,"memory":memory,"disk":disk(),"network":network,"uptime_seconds":fs::read_to_string("/proc/uptime").ok().and_then(|s|s.split_whitespace().next()?.parse::<f64>().ok()),"source":"Linux procfs/statvfs"})
    }
}
fn disk() -> Option<Value> {
    #[cfg(unix)]
    {
        let path = std::ffi::CString::new("/").ok()?;
        let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::statvfs(path.as_ptr(), st.as_mut_ptr()) } != 0 {
            return None;
        }
        let st = unsafe { st.assume_init() };
        Some(
            json!({"mount":"/","total_bytes":st.f_blocks*st.f_frsize,"available_bytes":st.f_bavail*st.f_frsize}),
        )
    }
    #[cfg(not(unix))]
    {
        None
    }
}
pub fn services(c: Option<&Config>) -> Value {
    json!(
        SERVICES
            .iter()
            .map(|name| {
                let enabled = c.is_some_and(|c| match *name {
                    "usque-cn" => c.routes_cn(),
                    "usque-warp" => c.warp.enabled,
                    "nodectl-share" => c.distributes(),
                    "librespeed" | "openlist" => c.serves_web(),
                    _ => true,
                });
                let state = host::command(
                    "systemctl",
                    &[
                        "show",
                        name,
                        "-p",
                        "ActiveState",
                        "-p",
                        "SubState",
                        "-p",
                        "LoadState",
                        "-p",
                        "NRestarts",
                    ],
                )
                .ok();
                let fields = state.map(|s| {
                    s.lines()
                        .filter_map(|l| l.split_once('='))
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect::<BTreeMap<_, _>>()
                });
                json!({"name":name,"enabled":enabled,"state":fields,"available":fields.is_some()})
            })
            .collect::<Vec<_>>()
    )
}
pub fn overview(s: &Store, m: &mut Meter) -> Value {
    let c = s.inputs().ok().map(|v| v.0);
    let current = s.pointer("current");
    json!({"version":env!("CARGO_PKG_VERSION"),"metrics":m.sample(),"current":current.as_ref().ok().and_then(|v|v.clone()),"integrity":current.is_ok(),"configured":c.is_some(),"domain":c.as_ref().map(|v|&v.node.domain),"services":services(c.as_ref()),"recovery_required":(["transaction.json","deployment.json","uninstall.json","admin-input.json"].iter().any(|n|s.state_dir.join(n).exists()))})
}
pub fn nodes(s: &Store) -> Result<Value> {
    let id = s.pointer("current")?.context("not_deployed")?;
    let (c, _) = s.verify(&id)?;
    Ok(
        json!({"release":id,"domain":c.node,"ports":c.ports,"distribution_enabled":c.distributes(),"access":share::access_status(s),"protocols":[{"name":"VLESS XHTTP","port":c.ports.https,"weight":c.clients.xhttp_weight},{"name":"VLESS WebSocket","port":c.ports.https,"weight":c.clients.ws_weight},{"name":"VLESS REALITY","port":c.ports.reality,"weight":c.clients.reality_weight}]}),
    )
}
pub fn network(s: &Store) -> Result<Value> {
    let (c, _) = s.inputs()?;
    Ok(
        json!({"native":c.node,"warp":c.warp,"cn_warp":c.cn_warp,"interfaces":host::command("ip",&["-json","address","show"]).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()),"routes_v4":host::command("ip",&["route","show","table","all"]).ok(),"routes_v6":host::command("ip",&["-6","route","show","table","all"]).ok(),"rules_v6":host::command("ip",&["-6","rule","show"]).ok(),"rules":host::command("ip",&["rule","show"]).ok(),"services":services(Some(&c))}),
    )
}
pub fn dns(s: &Store) -> Result<Value> {
    let (c, _) = s.inputs()?;
    let certs = [
        ("domain", &c.tls.domain_cert_dir),
        ("ip", &c.tls.ip_cert_dir),
    ]
    .iter()
    .map(|(kind, dir)| {
        let path = format!("{dir}/fullchain.pem");
        let details = host::command(
            "openssl",
            &[
                "x509",
                "-in",
                &path,
                "-noout",
                "-dates",
                "-ext",
                "subjectAltName",
                "-issuer",
            ],
        )
        .ok();
        let valid = host::command(
            "openssl",
            &["x509", "-in", &path, "-noout", "-checkend", "86400"],
        )
        .is_ok();
        json!({"kind":kind,"details":details,"valid_24h":valid})
    })
    .collect::<Vec<_>>();
    Ok(
        json!({"records":c.domains().iter().map(|d|json!({"name":d,"A":c.node.ipv4,"AAAA":c.node.ipv6,"proxied":false})).collect::<Vec<_>>(),"certificates":certs,"credentials_configured":s.config_dir.join("credentials/cloudflare-token").is_file(),"zone":fs::read_to_string(s.config_dir.join("credentials/cloudflare-zone")).ok()}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metric_parsers() {
        assert_eq!(cpu("cpu 10 20 30 40 5 6 7 8"), Some((126, 45)));
        assert_eq!(net("eth0: 12 0 0 0 0 0 0 0 34 0")["eth0"], (12, 34));
        assert!(cpu("bad").is_none());
    }
}
