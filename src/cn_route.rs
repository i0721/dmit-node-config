//! Only marked Xray sockets enter this table. Keep a rejecting default even
//! after usque exits: an absent TUN must never fall through to the main table.
use crate::{config, host, store};
use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, ValueEnum)]
pub enum Phase {
    Prepare,
    Ready,
    Block,
}

#[derive(Args)]
pub struct Command {
    #[arg(value_enum)]
    phase: Phase,
    #[command(flatten)]
    policy: Policy,
}

#[derive(Args, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[arg(long)]
    interface: String,
    #[arg(long)]
    table: u32,
    #[arg(long)]
    priority: u32,
    #[arg(long)]
    mark: u32,
}

impl Policy {
    pub(crate) fn guarded(&self) -> Result<bool> {
        if !self.inspect()? {
            return Ok(false);
        }
        // Do not let duplicate IPv4 rules hide a missing IPv6 guard (or vice
        // versa). Both families must independently retain their rejecting table
        // route and exactly one lookup plus one terminal mark rule.
        for family in ["-4", "-6"] {
            let (routes, rules) = self.family_state(family)?;
            if !self.family_guarded(&routes, &rules) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn family_guarded(&self, routes: &[Value], rules: &[Value]) -> bool {
        self.owned(routes, rules)
            && rules.len() == 2
            && rules
                .iter()
                .filter(|r| r["action"] == "unreachable")
                .count()
                == 1
            && routes.iter().any(|r| {
                r["type"] == "unreachable" && r["dst"] == "default" && r["metric"] == 32767
            })
    }
    pub(crate) fn without_routes(&self, serialized: &str) -> Result<String> {
        let mut routes: Vec<Value> = serde_json::from_str(serialized)?;
        routes.retain(|r| mark(r.get("table")) != Some(self.table));
        Ok(serde_json::to_string(&routes)?)
    }
    pub(crate) fn inspect(&self) -> Result<bool> {
        self.validate()?;
        let path = Path::new("/run/nodectl-cn-policy/owner.json");
        let (routes, rules) = self.state()?;
        if path.exists() {
            store::check_private(path, false)?;
            let saved: Self = serde_json::from_slice(&fs::read(path)?)?;
            ensure!(
                saved == *self && self.owned(&routes, &rules),
                "CN policy ownership or kernel rules differ"
            );
            Ok(true)
        } else {
            ensure!(
                routes.is_empty()
                    && rules.is_empty()
                    && !Path::new("/sys/class/net").join(&self.interface).exists(),
                "CN policy conflicts with an existing interface, mark, rule or table"
            );
            Ok(false)
        }
    }
    /// Only called after Xray using this policy has been stopped/replaced.
    pub(crate) fn release(&self) -> Result<()> {
        if !self.inspect()? {
            return Ok(());
        }
        self.reconcile(Phase::Block)?;
        let table = self.table.to_string();
        let priority = self.priority.to_string();
        let guard_priority = (self.priority + 1).to_string();
        let mask = format!("{}/0xffffffff", self.mark);
        for family in ["-4", "-6"] {
            let rules: Vec<Value> =
                serde_json::from_str(&self.ip(family, &["-j", "rule", "show"])?)?;
            if rules
                .iter()
                .any(|r| r["priority"].as_u64() == Some(self.priority.into()))
            {
                self.ip(
                    family,
                    &[
                        "rule", "del", "priority", &priority, "fwmark", &mask, "lookup", &table,
                    ],
                )?;
            }
            if rules
                .iter()
                .any(|r| r["priority"].as_u64() == Some((self.priority + 1).into()))
            {
                self.ip(
                    family,
                    &[
                        "rule",
                        "del",
                        "priority",
                        &guard_priority,
                        "fwmark",
                        &mask,
                        "unreachable",
                    ],
                )?;
            }
            let routes: Vec<Value> =
                serde_json::from_str(&self.ip(family, &["-j", "route", "show", "table", "all"])?)?;
            if routes.iter().any(|r| {
                mark(r.get("table")) == Some(self.table)
                    && r["type"] == "unreachable"
                    && r["metric"] == 32767
            }) {
                self.ip(
                    family,
                    &[
                        "route",
                        "del",
                        "unreachable",
                        "default",
                        "metric",
                        "32767",
                        "table",
                        &table,
                    ],
                )?;
            }
        }
        fs::remove_file("/run/nodectl-cn-policy/owner.json")?;
        Ok(())
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.interface.starts_with("warp-")
                && self.interface.len() <= 15
                && self
                    .interface
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Invalid CN interface"
        );
        ensure!(
            (1000..=30000).contains(&self.table)
                && (1000..=30000).contains(&self.priority)
                && (1024..=65535).contains(&self.mark),
            "Invalid CN routing identifiers"
        );
        Ok(())
    }
    fn ip(&self, family: &str, args: &[&str]) -> Result<String> {
        host::command("ip", &[&[family][..], args].concat())
    }
    fn state(&self) -> Result<(Vec<Value>, Vec<Value>)> {
        let mut routes = vec![];
        let mut rules = vec![];
        for family in ["-4", "-6"] {
            let (family_routes, family_rules) = self.family_state(family)?;
            routes.extend(family_routes);
            rules.extend(family_rules);
        }
        Ok((routes, rules))
    }
    fn family_state(&self, family: &str) -> Result<(Vec<Value>, Vec<Value>)> {
        let all: Vec<Value> =
            serde_json::from_str(&self.ip(family, &["-j", "route", "show", "table", "all"])?)?;
        let routes = all
            .into_iter()
            .filter(|r| mark(r.get("table")) == Some(self.table))
            .collect();
        let all: Vec<Value> = serde_json::from_str(&self.ip(family, &["-j", "rule", "show"])?)?;
        let rules = all
            .into_iter()
            .filter(|r| {
                r["priority"].as_u64() == Some(self.priority.into())
                    || r["priority"].as_u64() == Some((self.priority + 1).into())
                    || mark(r.get("table")) == Some(self.table)
                    || matches_mark(r, self.mark)
                    || (r["priority"]
                        .as_u64()
                        .is_some_and(|p| p > 0 && p < self.priority.into())
                        && r.get("fwmark").is_none())
            })
            .collect();
        Ok((routes, rules))
    }
    fn owned(&self, routes: &[Value], rules: &[Value]) -> bool {
        // Do not flush a table or delete an unrelated rule, even after drift.
        routes.iter().all(|r| {
            r["dst"] == "default"
                && ((r["type"] == "unreachable" && r["metric"] == 32767)
                    || (r["dev"] == self.interface && r["metric"] == 10))
                && r.get("gateway").is_none()
        }) && rules.iter().all(|r| {
            ((r["priority"].as_u64() == Some(self.priority.into())
                && mark(r.get("table")) == Some(self.table)
                && r.get("action").is_none())
                || (r["priority"].as_u64() == Some((self.priority + 1).into())
                    && r["action"] == "unreachable"
                    && r.get("table").is_none()))
                && mark(r.get("fwmark")) == Some(self.mark)
                && r["fwmask"].as_str().is_none_or(|s| s == "0xffffffff")
                && r.get("oif").is_none()
                && r.get("iif").is_none()
                && r["src"].as_str().is_none_or(|s| s == "all")
                && r.as_object().is_some_and(|o| {
                    o.keys().all(|k| {
                        [
                            "priority", "src", "table", "fwmark", "fwmask", "protocol", "action",
                        ]
                        .contains(&k.as_str())
                    })
                })
        })
    }
    fn reconcile(&self, phase: Phase) -> Result<()> {
        self.validate()?;
        let dir = Path::new("/run/nodectl-cn-policy");
        store::private_dir(dir)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)?;
        let owner = dir.join("owner.json");
        let (routes, rules) = self.state()?;
        if owner.exists() {
            store::check_private(&owner, false)?;
            let recorded: Policy = serde_json::from_slice(&fs::read(&owner)?)?;
            ensure!(
                recorded == *self && self.owned(&routes, &rules),
                "CN routing ownership or policy drifted"
            );
        } else {
            ensure!(
                matches!(phase, Phase::Prepare),
                "CN routing ownership is missing"
            );
            ensure!(
                routes.is_empty()
                    && rules.is_empty()
                    && !Path::new("/sys/class/net").join(&self.interface).exists(),
                "CN policy table, rule, mark or interface is already in use"
            );
            store::atomic_write(&owner, &serde_json::to_vec(self)?)?;
        }
        let table = self.table.to_string();
        let priority = self.priority.to_string();
        let guard_priority = (self.priority + 1).to_string();
        let mark = format!("{}/0xffffffff", self.mark);
        for family in ["-4", "-6"] {
            // Install rejection before adding a rule. Never reference main.
            self.ip(
                family,
                &[
                    "route",
                    "replace",
                    "unreachable",
                    "default",
                    "metric",
                    "32767",
                    "table",
                    &table,
                ],
            )?;
            let rules: Vec<Value> =
                serde_json::from_str(&self.ip(family, &["-j", "rule", "show"])?)?;
            // Keep a terminal mark guard for any lookup that falls through the
            // dedicated table. Bound-oif traffic also requires link quiescence.
            if !rules
                .iter()
                .any(|r| r["priority"].as_u64() == Some((self.priority + 1).into()))
            {
                self.ip(
                    family,
                    &[
                        "rule",
                        "add",
                        "priority",
                        &guard_priority,
                        "fwmark",
                        &mark,
                        "unreachable",
                    ],
                )?;
            }
            if !rules
                .iter()
                .any(|r| r["priority"].as_u64() == Some(self.priority.into()))
            {
                self.ip(
                    family,
                    &[
                        "rule", "add", "priority", &priority, "fwmark", &mark, "lookup", &table,
                    ],
                )?;
            }
        }
        if matches!(phase, Phase::Ready) {
            let started = Instant::now();
            loop {
                let ready = self
                    .ip("-j", &["link", "show", "dev", &self.interface])
                    .ok()
                    .and_then(|v| serde_json::from_str::<Vec<Value>>(&v).ok())
                    .is_some_and(|v| {
                        v.iter().any(|i| {
                            i["flags"]
                                .as_array()
                                .is_some_and(|flags| flags.iter().any(|f| f == "UP"))
                        })
                    });
                if ready {
                    break;
                }
                ensure!(
                    started.elapsed() < Duration::from_secs(10),
                    "CN TUN did not become ready"
                );
                thread::sleep(Duration::from_millis(100));
            }
            // Replies arrive on this dedicated TUN while the native route
            // remains in main. Scope loose RPF to our ephemeral link only.
            let interface_rpf = format!("/proc/sys/net/ipv4/conf/{}/rp_filter", self.interface);
            let global: u8 = fs::read_to_string("/proc/sys/net/ipv4/conf/all/rp_filter")?
                .trim()
                .parse()?;
            let current: u8 = fs::read_to_string(&interface_rpf)?.trim().parse()?;
            ensure!(
                global <= 2 && current <= 2,
                "Unsupported reverse-path filtering state"
            );
            if global.max(current) == 1 {
                fs::write(&interface_rpf, b"2\n").context("Strict reverse-path filtering requires loose RPF on the dedicated CN link only")?;
            }
            for family in ["-4", "-6"] {
                self.ip(
                    family,
                    &[
                        "route",
                        "replace",
                        "default",
                        "dev",
                        &self.interface,
                        "metric",
                        "10",
                        "table",
                        &table,
                    ],
                )?;
            }
        } else if matches!(phase, Phase::Block) {
            // Linux can construct an on-link route for a bound oif even when
            // RPDB rejects ordinary marked lookups. Quiesce only our own link;
            // rules remain in place if the interface is absent or recreated.
            if self
                .ip("-j", &["link", "show", "dev", &self.interface])
                .is_ok()
            {
                host::command("ip", &["link", "set", "dev", &self.interface, "down"])?;
            }
            for family in ["-4", "-6"] {
                let routes: Vec<Value> = serde_json::from_str(
                    &self.ip(family, &["-j", "route", "show", "table", &table])?,
                )?;
                if routes
                    .iter()
                    .any(|r| r["dev"] == self.interface && r["metric"] == 10)
                {
                    self.ip(
                        family,
                        &[
                            "route",
                            "del",
                            "default",
                            "dev",
                            &self.interface,
                            "metric",
                            "10",
                            "table",
                            &table,
                        ],
                    )?;
                }
            }
        }
        Ok(())
    }
}

fn mark(value: Option<&Value>) -> Option<u32> {
    value.and_then(|v| {
        v.as_u64().and_then(|n| n.try_into().ok()).or_else(|| {
            let text = v.as_str()?;
            if let Some(hex) = text.strip_prefix("0x") {
                u32::from_str_radix(hex, 16).ok()
            } else {
                text.parse().ok()
            }
        })
    })
}

fn matches_mark(rule: &Value, candidate: u32) -> bool {
    mark(rule.get("fwmark")).is_some_and(|value| {
        let mask = mark(rule.get("fwmask")).unwrap_or(u32::MAX);
        candidate & mask == value & mask
    })
}

pub fn run(command: Command) -> Result<()> {
    host::linux_root()?;
    command
        .policy
        .reconcile(command.phase)
        .context("CN route policy failed closed")
}

impl From<&config::CnWarp> for Policy {
    fn from(c: &config::CnWarp) -> Self {
        Self {
            interface: c.interface.clone(),
            table: c.routing_table,
            priority: c.rule_priority,
            mark: c.socket_mark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "Requires explicit fresh mount/network namespaces and private /run"]
    fn kernel_guard_rejects_missing_family_rules_and_reject_route() {
        assert_eq!(
            std::env::var("NODECTL_CN_POLICY_TEST").as_deref(),
            Ok("isolated")
        );
        for namespace in ["net", "mnt"] {
            assert_ne!(
                fs::read_link(format!("/proc/self/ns/{namespace}")).unwrap(),
                fs::read_link(format!("/proc/1/ns/{namespace}")).unwrap()
            );
        }
        let links: Vec<Value> =
            serde_json::from_str(&host::command("ip", &["-j", "link", "show"]).unwrap()).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["ifname"], "lo");
        let p = Policy::from(&config::CnWarp::managed(true));
        assert!(!p.inspect().unwrap());
        p.reconcile(Phase::Prepare).unwrap();
        assert!(p.guarded().unwrap());
        for (priority, suffix) in [
            ("15271", vec!["lookup", "5271"]),
            ("15272", vec!["unreachable"]),
        ] {
            let base = ["rule", "del", "priority", priority, "fwmark", "5271"];
            p.ip("-6", &[base.as_slice(), &suffix].concat()).unwrap();
        }
        assert_eq!(p.state().unwrap().1.len(), 2);
        assert!(!p.guarded().unwrap());
        p.reconcile(Phase::Prepare).unwrap();
        assert!(p.guarded().unwrap());
        p.ip(
            "-6",
            &[
                "route",
                "del",
                "unreachable",
                "default",
                "metric",
                "32767",
                "table",
                "5271",
            ],
        )
        .unwrap();
        assert!(!p.guarded().unwrap());
        p.reconcile(Phase::Prepare).unwrap();
        assert!(p.guarded().unwrap());
        p.release().unwrap();
        println!("CN_KERNEL_PER_FAMILY_MISSING_RULES_AND_REJECT_GUARD_PASS");
    }
    #[test]
    fn each_family_needs_its_own_reject_route_lookup_and_terminal_guard() {
        let p = Policy::from(&config::CnWarp::managed(true));
        let route = serde_json::json!({"dst":"default","type":"unreachable","metric":32767});
        let lookup =
            serde_json::json!({"priority":15271,"table":5271,"src":"all","fwmark":"0x1497"});
        let terminal = serde_json::json!({"priority":15272,"action":"unreachable","src":"all","fwmark":"0x1497"});
        let routes = [route];
        let rules = [lookup.clone(), terminal.clone()];
        assert!(p.family_guarded(&routes, &rules));
        assert!(!p.family_guarded(&[], &rules));
        assert!(!p.family_guarded(&routes, &[]));
        assert!(!p.family_guarded(&routes, &[lookup.clone(), lookup.clone()]));
        assert!(!p.family_guarded(&routes, &[terminal.clone(), terminal.clone()]));
        // A combined count of four can hide all rules being in one family.
        assert!(!p.family_guarded(
            &routes,
            &[lookup.clone(), terminal.clone(), lookup, terminal]
        ));
    }
    #[test]
    fn foreign_routes_rules_and_global_tables_are_rejected() {
        let mut p = Policy {
            interface: "warp-cn".into(),
            table: 5271,
            priority: 15271,
            mark: 5271,
        };
        assert!(p.validate().is_ok());
        let good = serde_json::json!({"dst":"default","type":"unreachable","metric":32767});
        assert!(p.owned(std::slice::from_ref(&good), &[]));
        assert!(p.owned(
            std::slice::from_ref(&good),
            &[serde_json::json!({
                "priority":15271,"table":"5271","src":"all","fwmark":"0x1497"
            })]
        ));
        assert!(!p.owned(
            &[serde_json::json!({"dst":"default","dev":"eth0","metric":10})],
            &[]
        ));
        assert!(!p.owned(
            &[good],
            &[serde_json::json!({"priority":15271,"table":5271,"src":"all"})]
        ));
        p.table = 254;
        assert!(p.validate().is_err());
        assert!(matches_mark(
            &serde_json::json!({"fwmark":"0x1000","fwmask":"0xf000"}),
            5271
        ));
    }
}
