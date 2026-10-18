use crate::config::{Config, Secrets};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const TEMPLATE_VERSION: &str = "4";

pub fn template(source: &str, vars: &BTreeMap<&str, String>) -> Result<String> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    let result = env.template_from_str(source)?.render(vars)?;
    ensure!(!result.contains("@@"), "Unresolved template variable");
    Ok(result)
}

fn inbound(tag: &str, id: &str, stream: Value) -> Value {
    json!({"tag":tag,"protocol":"vless","settings":{"clients":[{"id":id}],"decryption":"none"},"streamSettings":stream})
}

pub fn xray(c: &Config, s: &Secrets) -> Value {
    let mut xhttp = inbound(
        "vless-xhttp",
        &s.xhttp_uuid,
        json!({"network":"xhttp","xhttpSettings":{"path":c.transport.xhttp.path,"mode":c.transport.xhttp.mode}}),
    );
    xhttp["listen"] = json!(format!("{},0660", c.transport.xhttp.socket));
    let mut ws = inbound(
        "vless-ws",
        &s.ws_uuid,
        json!({"network":"ws","wsSettings":{"path":c.transport.ws.path}}),
    );
    ws["listen"] = json!(format!("{},0660", c.transport.ws.socket));
    let mut reality = inbound(
        "vless-reality",
        &s.reality_uuid,
        json!({"network":"xhttp","security":"reality",
        "xhttpSettings":{"path":c.transport.reality.path,"mode":c.transport.reality.mode},
        "realitySettings":{"show":false,"target":format!("127.0.0.1:{}", c.ports.https),"xver":0,
            "serverNames":c.sans(),"privateKey":s.reality_private_key,"shortIds":[s.reality_short_id]}}),
    );
    reality["listen"] = json!("0.0.0.0");
    reality["port"] = json!(c.ports.reality);
    let mut inbounds = vec![xhttp, ws, reality.clone()];
    if !c.node.ipv6.is_empty() {
        reality["tag"] = json!("vless-reality-ipv6");
        reality["listen"] = json!("::");
        // Xray's IPv6 listener is explicitly v6-only to avoid colliding with IPv4.
        reality["streamSettings"]["sockopt"] = json!({"v6only":true});
        inbounds.push(reality);
    }
    if c.encrypted() {
        let keys = s.vless.as_ref().expect("validated encryption keys");
        for inbound in &mut inbounds {
            let kind = if inbound["tag"] == "vless-xhttp" {
                "xhttp"
            } else if inbound["tag"] == "vless-ws" {
                "ws"
            } else {
                "reality"
            };
            inbound["settings"]["decryption"] = json!(keys.pair(kind).decryption);
            inbound["settings"]["clients"][0]["flow"] = json!("xtls-rprx-vision");
        }
    }
    let direct = json!({"tag":"direct","protocol":"freedom"});
    // The isolated HTTPS fixture is local. Xray blocks private destinations by
    // default; allow only this test endpoint without weakening release output.
    #[cfg(feature = "integration-test")]
    let direct = {
        let mut direct = direct;
        direct["settings"] = json!({"finalRules":[{"action":"allow","ip":["127.0.0.1/32"],"port":"9443"},{"action":"allow","ip":["198.18.0.2/32"],"port":"53,443,14444"}]});
        direct
    };
    let mut rendered = json!({"log":{"loglevel":"warning","access":format!("{}/xray/access.log",c.runtime.log_root),
        "error":format!("{}/xray/error.log",c.runtime.log_root)},"inbounds":inbounds,
        "outbounds":[direct,{"tag":"block","protocol":"blackhole"}]});
    crate::cn_warp::xray(c, &mut rendered);
    rendered
}

pub fn outbound(c: &Config, s: &Secrets, kind: &str, fingerprint: &str) -> Result<Value> {
    let (id, port, path, security, sni, network) = match kind {
        "xhttp" => (
            &s.xhttp_uuid,
            c.ports.https,
            &c.transport.xhttp.path,
            "tls",
            &c.node.domain,
            "xhttp",
        ),
        "ws" => (
            &s.ws_uuid,
            c.ports.https,
            &c.transport.ws.path,
            "tls",
            &c.node.domain,
            "ws",
        ),
        "reality" => (
            &s.reality_uuid,
            c.ports.reality,
            &c.transport.reality.path,
            "reality",
            &c.node.reality_domain,
            "xhttp",
        ),
        _ => anyhow::bail!("Unknown profile type"),
    };
    let mut stream = json!({"network":network,"security":security});
    if network == "ws" {
        stream["wsSettings"] = json!({"path":path,"headers":{"Host":c.node.domain}});
    } else {
        stream["xhttpSettings"] = json!({"path":path,"mode":"stream-one"});
    }
    if security == "tls" {
        // stream-one uses HTTP/2. h3 is advertised by Nginx, but is not forced on
        // a TCP uTLS client that cannot implement QUIC merely by selecting ALPN.
        stream["tlsSettings"] = json!({"serverName":sni,"fingerprint":fingerprint,"allowInsecure":false,
            "alpn":if network == "ws" {vec!["http/1.1"]} else {vec!["h2", "http/1.1"]},"minVersion":"1.3"});
    } else {
        stream["realitySettings"] = json!({"serverName":sni,"fingerprint":fingerprint,
            "password":s.public_key()?,"shortId":s.reality_short_id});
    }
    let mut profile = json!({"tag":format!("{kind}-{fingerprint}"),"protocol":"vless",
        "settings":{"vnext":[{"address":c.node.domain,"port":port,"users":[{"id":id,"encryption":"none"}]}]},
        "streamSettings":stream});
    if c.encrypted() {
        profile["settings"]["vnext"][0]["users"][0]["encryption"] = json!(
            s.vless
                .as_ref()
                .expect("validated encryption keys")
                .pair(kind)
                .encryption
        );
        profile["settings"]["vnext"][0]["users"][0]["flow"] = json!("xtls-rprx-vision-udp443");
    }
    Ok(profile)
}

/// JSON is a YAML 1.2 subset accepted by Mihomo; use one safe serializer,
/// including for arbitrary operator-supplied paths, instead of interpolating YAML.
pub fn mihomo(c: &Config, s: &Secrets) -> Result<Vec<u8>> {
    let profiles = profiles(c, s)?;
    let mut proxies = vec![];
    for p in profiles["profiles"].as_array().unwrap() {
        let o = &p["outbound"];
        let stream = &o["streamSettings"];
        let user = &o["settings"]["vnext"][0]["users"][0];
        let real = stream["security"] == "reality";
        let tls = if real {
            &stream["realitySettings"]
        } else {
            &stream["tlsSettings"]
        };
        let mut node = json!({"name":p["name"],"type":"vless","server":o["settings"]["vnext"][0]["address"],"port":o["settings"]["vnext"][0]["port"],"uuid":user["id"],"udp":true,"packet-encoding":"xudp","tls":true,"servername":tls["serverName"],"client-fingerprint":tls["fingerprint"],"skip-cert-verify":false,"network":stream["network"]});
        if stream["network"] == "ws" {
            node["alpn"] = json!(["http/1.1"]);
            node["ws-opts"] = stream["wsSettings"].clone();
        } else {
            node["alpn"] = json!(["h2", "http/1.1"]);
            node["xhttp-opts"] = stream["xhttpSettings"].clone();
        }
        if real {
            // The pinned REALITY server requires the hybrid key share. Mihomo
            // otherwise strips it from its fingerprint by default.
            node["reality-opts"] = json!({"public-key":tls["password"],"short-id":tls["shortId"],"support-x25519mlkem768":true});
        }
        if c.encrypted() {
            node["encryption"] = user["encryption"].clone();
            node["flow"] = json!("xtls-rprx-vision");
        }
        proxies.push(node);
    }
    serde_json::to_vec_pretty(&json!({"proxies":proxies})).map_err(Into::into)
}

pub fn profiles(c: &Config, s: &Secrets) -> Result<Value> {
    let total: u64 = c
        .clients
        .fingerprint_weights
        .values()
        .map(|&n| u64::from(n))
        .sum();
    let mut profiles = vec![];
    for (fp, &weight) in &c.clients.fingerprint_weights {
        if weight > 0 {
            profiles.push(json!({"name":format!("xhttp-{fp}"),"weight":{"numerator":u64::from(c.clients.xhttp_weight)*u64::from(weight),"denominator":100*total},
                "fingerprint_copies":weight,"outbound":outbound(c,s,"xhttp",fp)?}));
        }
    }
    for (kind, weight) in [
        ("ws", c.clients.ws_weight),
        ("reality", c.clients.reality_weight),
    ] {
        profiles.push(json!({"name":kind,"weight":{"numerator":weight,"denominator":100},"outbound":outbound(c,s,kind,"chrome")?}));
    }
    let allocation: Vec<_> = c
        .clients
        .fingerprint_weights
        .iter()
        .flat_map(|(name, &count)| std::iter::repeat_n(format!("xhttp-{name}"), count as usize))
        .collect();
    Ok(
        json!({"format":"nodectl-xray-outbounds-v1","note":"Client allocation weights; no server-side random routing.",
        "xhttp_allocation":allocation,"profiles":profiles}),
    )
}

pub fn files(c: &Config, s: &Secrets) -> Result<BTreeMap<String, Vec<u8>>> {
    files_with_geodata(c, s, None)
}

pub fn files_with_geodata(
    c: &Config,
    s: &Secrets,
    geodata: Option<&std::path::Path>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let p = &c.ports;
    let addresses = if c.node.ipv6.is_empty() {
        vec!["".to_owned()]
    } else {
        vec!["".to_owned(), "[::]:".to_owned()]
    };
    let listeners = |default: bool| {
        let mut lines = vec![];
        for addr in &addresses {
            let suffix = if default { " default_server" } else { "" };
            lines.push(format!("        listen {addr}{} ssl{suffix};", p.https));
            if c.tls.http3 {
                lines.push(format!(
                    "        listen {addr}{} quic{}{suffix};",
                    p.https,
                    if default { " reuseport" } else { "" }
                ));
            }
        }
        lines.join("\n")
    };
    let mut vars = BTreeMap::from([
        ("NGINX_USER", c.runtime.nginx_user.clone()),
        ("XRAY_USER", c.runtime.xray_user.clone()),
        ("LOG_ROOT", c.runtime.log_root.clone()),
        ("HTTP3", if c.tls.http3 { "http3 on;" } else { "" }.into()),
        (
            "HTTP_LISTEN",
            addresses
                .iter()
                .map(|a| format!("        listen {a}{} default_server;", p.http))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("DEFAULT_LISTEN", listeners(true)),
        ("DOMAIN_LISTEN", listeners(false)),
        (
            "REDIRECT_PORT",
            if p.https == 443 {
                String::new()
            } else {
                format!(":{}", p.https)
            },
        ),
        ("IP_CERT", c.tls.ip_cert_dir.clone()),
        ("DOMAIN_CERT", c.tls.domain_cert_dir.clone()),
        (
            "ALT_SVC",
            if c.tls.http3 {
                format!(
                    "add_header Alt-Svc 'h3=\":{}\"; ma={}' always;",
                    p.https, c.tls.alt_svc_max_age
                )
            } else {
                String::new()
            },
        ),
        ("WEB_ROOT", c.website.root.clone()),
        ("DOMAINS", c.domains().join(" ")),
        ("XHTTP_PATH", c.transport.xhttp.path.clone()),
        ("WS_PATH", c.transport.ws.path.clone()),
        ("XHTTP_SOCKET", c.transport.xhttp.socket.clone()),
        ("WS_SOCKET", c.transport.ws.socket.clone()),
        (
            "SHARE_LOG_FORMAT",
            if c.distribution.is_some() {
                "log_format node_safe '$remote_addr [$time_local] \"$request_method $uri\" $status $body_bytes_sent';".into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_LOG_TYPE",
            if c.distribution.is_some() {
                "node_safe".into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_HTTP",
            if c.distribution.is_some() {
                "error_log /dev/null;\n        location ^~ /nodes/ { access_log off; return 404; }"
                    .into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_ERROR",
            if c.distribution.is_some() {
                "error_log /dev/null;".into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_LIMIT",
            if c.distributes() {
                "limit_req_zone $binary_remote_addr zone=node_distribution:1m rate=5r/s;\n    limit_conn_zone $binary_remote_addr zone=node_distribution_connections:1m;".into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_ROUTE",
            if c.distributes() {
                crate::share_http::nginx_route()
            } else if c.distribution.is_some() {
                "location ^~ /nodes/ { access_log off; return 404; }".into()
            } else {
                String::new()
            },
        ),
        (
            "SHARE_IP",
            if c.distribution.is_some() {
                "location ^~ /nodes/ { access_log off; return 404; }".into()
            } else {
                String::new()
            },
        ),
        ("XRAY_BINARY", c.runtime.xray_binary.clone()),
        ("NGINX_BINARY", c.runtime.nginx_binary.clone()),
        ("USQUE_BINARY", c.runtime.usque_binary.clone()),
        ("WARP_CONFIG", c.warp.config_file.clone()),
        (
            "WARP_DIR",
            std::path::Path::new(&c.warp.config_file)
                .parent()
                .unwrap()
                .display()
                .to_string(),
        ),
    ]);
    crate::web_services::nginx(c, &mut vars)?;
    vars.insert(
        "CN_UNIT",
        if c.routes_cn() {
            "Wants=usque-cn.service\nAfter=usque-cn.service"
        } else {
            ""
        }
        .into(),
    );
    vars.insert("CN_ASSETS", if c.routes_cn() {
        format!("Environment=XRAY_LOCATION_ASSET={}\nCapabilityBoundingSet=CAP_NET_RAW\nAmbientCapabilities=CAP_NET_RAW", crate::cn_warp::runtime_asset_dir()?.display())
    } else {String::new()});
    let operations = operations(c);
    let mut files = BTreeMap::from([
        ("xray.json".into(), serde_json::to_vec_pretty(&xray(c, s))?),
        (
            "clients/profiles.json".into(),
            serde_json::to_vec_pretty(&profiles(c, s)?)?,
        ),
        (
            "operations.json".into(),
            serde_json::to_vec_pretty(&operations)?,
        ),
        (
            "source/node.toml".into(),
            toml::to_string_pretty(c)?.into_bytes(),
        ),
        ("source/secrets.json".into(), serde_json::to_vec_pretty(s)?),
    ]);
    for (name, source) in [
        ("nginx.conf", include_str!("../templates/nginx.conf.tpl")),
        (
            "systemd/xray.service",
            include_str!("../templates/xray.v4.service.tpl"),
        ),
        (
            "systemd/nginx.service",
            include_str!("../templates/nginx.service.tpl"),
        ),
        (
            "systemd/usque-warp.service",
            include_str!("../templates/usque-warp.service.tpl"),
        ),
    ] {
        files.insert(name.into(), template(source, &vars)?.into_bytes());
    }
    if c.distribution.is_some() {
        files.insert(
            "systemd/nodectl-share.service".into(),
            include_bytes!("../templates/nodectl-share.service.tpl").to_vec(),
        );
    }
    crate::cn_warp::files(c, &mut files, geodata)?;
    crate::web_services::files(c, &mut files)?;
    Ok(files)
}

/// Shared non-secret deployment operations, usable before key initialization.
pub fn operations(c: &Config) -> Value {
    let p = &c.ports;
    let mut domain_issue = vec![
        "--issue".to_owned(),
        "--server".into(),
        "letsencrypt".into(),
        "--dns".into(),
        "DNS_PROVIDER".into(),
        "--keylength".into(),
        "ec-384".into(),
    ];
    domain_issue.extend(["-d".into(), c.node.domain.clone()]);
    for domain in c.domains().into_iter().filter(|d| d != &c.node.domain) {
        domain_issue.extend(["-d".into(), domain]);
    }
    let mut ip_issue = vec![
        "--issue".to_owned(),
        "--server".into(),
        "letsencrypt".into(),
        "--webroot".into(),
        "/var/www/acme".into(),
        "--certificate-profile".into(),
        "shortlived".into(),
        "--days".into(),
        "3".into(),
        "--keylength".into(),
        "ec-256".into(),
    ];
    for ip in c.ips() {
        ip_issue.extend(["-d".into(), ip]);
    }
    let install = |domain: &str, dir: &str| {
        vec![
            "--install-cert".to_owned(),
            "--ecc".into(),
            "-d".into(),
            domain.into(),
            "--key-file".into(),
            format!("{dir}/privkey.pem"),
            "--fullchain-file".into(),
            format!("{dir}/fullchain.pem"),
            "--reloadcmd".into(),
            "/usr/local/bin/nodectl renew-hook".into(),
        ]
    };
    json!({"schema_version":1,"template_version":TEMPLATE_VERSION,
        "dns":c.domains().iter().flat_map(|d| c.ips().iter().map(|ip| json!({"name":d,"type":if ip.contains(':') {"AAAA"} else {"A"},"value":ip})).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "certificates":{"domain":{"sans":c.domains(),"keylength":"ec-384","directory":c.tls.domain_cert_dir,
            "acme_issue_args":domain_issue,"acme_install_args":install(&c.node.domain,&c.tls.domain_cert_dir)},
            "ip":{"sans":c.ips(),"profile":"shortlived","directory":c.tls.ip_cert_dir,
                "acme_issue_args":ip_issue,"acme_install_args":install(&c.node.ipv4,&c.tls.ip_cert_dir)}},
        "listeners":{"tcp": [p.http,p.https,p.reality],"udp":if c.tls.http3 {vec![p.https]} else {vec![]}},
        "uds":[c.transport.xhttp.socket,c.transport.ws.socket],"reality_target":format!("127.0.0.1:{}",p.https),
        "addresses":c.ips(),"warp":{"enabled":c.warp.enabled,"config_file":c.warp.config_file,"take_over_default_route":false,
            "registration_command":[c.runtime.usque_binary,"-c",c.warp.config_file,"register"]},
        "warnings":["REALITY with a local Nginx target requires a real handshake check.","DNS, certificate issuance, binary installation and WARP registration are separate provisioning steps."],
        "directories":[{"path":format!("{}/xray",c.runtime.log_root),"owner":c.runtime.xray_user,"group":c.runtime.nginx_user,"mode":"0750"},
            {"path":format!("{}/nginx",c.runtime.log_root),"owner":"root","mode":"0755"}],
        "client_transport_weights":{"xhttp":c.clients.xhttp_weight,"ws":c.clients.ws_weight,"reality":c.clients.reality_weight}})
}
