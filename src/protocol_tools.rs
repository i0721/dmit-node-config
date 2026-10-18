//! Isolated acceptance tools. Never linked into the production nodectl binary.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    path::Path,
    time::Duration,
};

fn private_write(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn legacy_inputs(root: &Path) -> Result<()> {
    let mut config: toml::Value = toml::from_str(&fs::read_to_string(root.join("node.toml"))?)?;
    config
        .as_table_mut()
        .context("Config table")?
        .remove("vless");
    config.as_table_mut().unwrap().remove("distribution");
    fs::write(root.join("node.toml"), toml::to_string_pretty(&config)?)?;
    let mut secrets: Value = serde_json::from_slice(&fs::read(root.join("secrets.json"))?)?;
    secrets
        .as_object_mut()
        .context("Secrets object")?
        .remove("vless");
    private_write(&root.join("secrets.json"), &secrets)
}

pub fn configs(
    root: &Path,
    engine: &str,
    kind: &str,
    server: &str,
    output: &Path,
    negative: Option<&str>,
) -> Result<()> {
    let _: std::net::Ipv4Addr = server.parse()?;
    ensure!(
        ["xray", "mihomo"].contains(&engine),
        "Unknown client engine"
    );
    fs::create_dir_all(output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(output, fs::Permissions::from_mode(0o700))?;
    }
    let profiles: Value = serde_json::from_slice(&fs::read(root.join("profiles.json"))?)?;
    let profile = profiles["profiles"]
        .as_array()
        .context("Profiles")?
        .iter()
        .find(|p| p["name"] == kind)
        .context("Missing profile")?;
    let mut outbound = profile["outbound"].clone();
    outbound["settings"]["vnext"][0]["address"] = json!(server);
    let capture = std::env::var("NODECTL_CAPTURE_HELLO").as_deref() == Ok("yes");
    if capture {
        private_write(
            &output.join("relay.json"),
            &json!({"remote":format!("{server}:{}", outbound["settings"]["vnext"][0]["port"].as_u64().context("Port")?)}),
        )?;
        outbound["settings"]["vnext"][0]["address"] = json!("127.0.0.1");
        outbound["settings"]["vnext"][0]["port"] = json!(18443);
    }
    if outbound["streamSettings"]["security"] == "tls" {
        outbound["streamSettings"]["tlsSettings"]["useSystemCA"] = json!(true);
    }
    if let Some(negative) = negative {
        let user = &mut outbound["settings"]["vnext"][0]["users"][0];
        match negative {
            "missing-encryption" => user["encryption"] = json!("none"),
            "legacy" => {
                user["encryption"] = json!("none");
                user.as_object_mut().unwrap().remove("flow");
            }
            "wrong-flow" => user["flow"] = json!(""),
            "wrong-key" => {
                // A valid parameter from another independent entrypoint must fail.
                let other = profiles["profiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["name"] == if kind == "ws" { "reality" } else { "ws" })
                    .context("Other profile")?;
                user["encryption"] =
                    other["outbound"]["settings"]["vnext"][0]["users"][0]["encryption"].clone();
            }
            _ => bail!("Unknown negative fixture"),
        }
    }
    private_write(
        &output.join("xray.json"),
        &json!({"log":{"loglevel":"warning"},"inbounds":[{"listen":"127.0.0.1","port":10810,"protocol":"socks","settings":{"udp":true,"auth":"noauth"}}],"outbounds":[outbound]}),
    )?;
    let nodes: Value = serde_json::from_slice(&fs::read(root.join("proxies.yaml"))?)?;
    let node = nodes["proxies"]
        .as_array()
        .context("Mihomo nodes")?
        .iter()
        .find(|p| p["name"] == kind)
        .context("Missing Mihomo node")?;
    let mut node = if engine == "xray" {
        json!({"type":"socks5","server":"127.0.0.1","port":10810,"udp":true})
    } else {
        node.clone()
    };
    if engine == "mihomo" {
        node["server"] = json!(if capture { "127.0.0.1" } else { server });
        if capture {
            node["port"] = json!(18443);
        }
        if let Some(negative) = negative {
            match negative {
                "missing-encryption" => node["encryption"] = json!("none"),
                "legacy" => {
                    node["encryption"] = json!("none");
                    node.as_object_mut().unwrap().remove("flow");
                }
                "wrong-flow" => node["flow"] = json!(""),
                "wrong-key" => {
                    node["encryption"] = nodes["proxies"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| p["name"] == if kind == "ws" { "reality" } else { "ws" })
                        .context("Other Mihomo node")?["encryption"]
                        .clone();
                }
                _ => bail!("Unknown negative fixture"),
            }
        }
    }
    node["name"] = json!("verification");
    private_write(
        &output.join("mihomo.yaml"),
        &json!({"mixed-port":10811,"allow-lan":false,"bind-address":"127.0.0.1","log-level":"warning","ipv6":false,"tun":{"enable":true,"device":"verify0","stack":"gvisor","auto-route":false,"auto-detect-interface":false},"dns":{"enable":false},"proxies":[node],"rules":["MATCH,verification"]}),
    )
}

// A byte-for-byte TCP relay records only public ClientHello metadata. It does
// not terminate TLS, record payloads or change either engine's handshake.
pub fn hello_relay(root: &Path) -> Result<()> {
    let config: Value = serde_json::from_slice(&fs::read(root.join("relay.json"))?)?;
    let remote: SocketAddr = config["remote"].as_str().context("Relay remote")?.parse()?;
    for stream in TcpListener::bind("127.0.0.1:18443")?.incoming() {
        let mut client = stream?;
        let root = root.to_owned();
        std::thread::spawn(move || -> Result<()> {
            client.set_read_timeout(Some(Duration::from_secs(30)))?;
            let mut header = [0; 5];
            client.read_exact(&mut header)?;
            ensure!(header[0] == 22, "Expected TLS handshake");
            let mut hello = vec![0; u16::from_be_bytes([header[3], header[4]]) as usize];
            client.read_exact(&mut hello)?;
            private_write(&root.join("hello.json"), &hello_metadata(&hello)?)?;
            let mut upstream = TcpStream::connect_timeout(&remote, Duration::from_secs(10))?;
            upstream.write_all(&header)?;
            upstream.write_all(&hello)?;
            client.set_read_timeout(None)?;
            let mut write_client = client.try_clone()?;
            let mut read_upstream = upstream.try_clone()?;
            let downstream = std::thread::spawn(move || {
                let _ = std::io::copy(&mut read_upstream, &mut write_client);
                let _ = write_client.shutdown(std::net::Shutdown::Write);
            });
            let _ = std::io::copy(&mut client, &mut upstream);
            let _ = upstream.shutdown(std::net::Shutdown::Write);
            let _ = downstream.join();
            Ok(())
        });
    }
    Ok(())
}
fn hello_metadata(hello: &[u8]) -> Result<Value> {
    fn skip<'a>(bytes: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
        ensure!(bytes.len() >= n, "Truncated ClientHello");
        let (head, tail) = bytes.split_at(n);
        *bytes = tail;
        Ok(head)
    }
    fn word(bytes: &mut &[u8]) -> Result<usize> {
        let a = skip(bytes, 2)?;
        Ok(u16::from_be_bytes([a[0], a[1]]) as usize)
    }
    let mut bytes = hello;
    ensure!(skip(&mut bytes, 1)?[0] == 1, "Expected ClientHello");
    skip(&mut bytes, 3 + 2 + 32)?;
    let n = skip(&mut bytes, 1)?[0] as usize;
    skip(&mut bytes, n)?;
    let n = word(&mut bytes)?;
    skip(&mut bytes, n)?;
    let n = skip(&mut bytes, 1)?[0] as usize;
    skip(&mut bytes, n)?;
    let n = word(&mut bytes)?;
    let mut extensions = skip(&mut bytes, n)?;
    let mut alpn = vec![];
    let mut shares = vec![];
    while !extensions.is_empty() {
        let kind = word(&mut extensions)?;
        let n = word(&mut extensions)?;
        let mut value = skip(&mut extensions, n)?;
        if kind == 16 {
            let n = word(&mut value)?;
            let mut list = skip(&mut value, n)?;
            while !list.is_empty() {
                let n = skip(&mut list, 1)?[0] as usize;
                alpn.push(std::str::from_utf8(skip(&mut list, n)?)?.to_owned());
            }
        } else if kind == 51 {
            let n = word(&mut value)?;
            let mut list = skip(&mut value, n)?;
            while !list.is_empty() {
                shares.push(word(&mut list)?);
                let n = word(&mut list)?;
                skip(&mut list, n)?;
            }
        }
    }
    Ok(json!({"alpn":alpn,"key_shares":shares}))
}

pub fn hello_check(root: &Path, kind: &str) -> Result<()> {
    let hello: Value = serde_json::from_slice(&fs::read(root.join("hello.json"))?)?;
    if kind == "ws" {
        ensure!(
            hello["alpn"] == json!(["http/1.1"]),
            "WS ClientHello ALPN differs"
        );
    } else if kind == "reality" {
        let shares = hello["key_shares"].as_array().context("Key shares")?;
        let hybrid = shares
            .iter()
            .position(|v| v == 4588)
            .context("Missing hybrid key share")?;
        ensure!(
            shares
                .iter()
                .position(|v| v == 29)
                .is_none_or(|i| hybrid < i),
            "Hybrid share ordering differs"
        );
        ensure!(
            hello["alpn"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("h2"))),
            "REALITY fingerprint lacks h2"
        );
    } else {
        ensure!(
            hello["alpn"] == json!(["h2", "http/1.1"]),
            "XHTTP TLS ClientHello ALPN differs"
        );
    }
    println!(
        "CLIENT_HELLO_ALPN_KEY_SHARE_PASS:{kind}:{}",
        serde_json::to_string(&hello)?
    );
    Ok(())
}

pub fn echo(address: &str, dns: bool) -> Result<()> {
    let socket = UdpSocket::bind(address)?;
    let mut buf = [0u8; 65535];
    loop {
        let (n, peer) = socket.recv_from(&mut buf)?;
        if dns {
            ensure!(n >= 12, "Truncated DNS query");
            let mut answer = buf[..n].to_vec();
            answer[2] = 0x81;
            answer[3] = 0x80;
            answer[6] = 0;
            answer[7] = 1;
            answer.extend([0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 203, 0, 113, 1]);
            socket.send_to(&answer, peer)?;
        } else {
            socket.send_to(&buf[..n], peer)?;
        }
    }
}

pub fn udp(socks: u16, target: IpAddr, port: u16, mode: &str) -> Result<()> {
    ensure!(
        ["dns", "echo", "stun"].contains(&mode),
        "Unknown UDP probe mode"
    );
    let mut tcp = TcpStream::connect(("127.0.0.1", socks))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(10)))?;
    tcp.write_all(&[5, 1, 0])?;
    let mut auth = [0; 2];
    tcp.read_exact(&mut auth)?;
    ensure!(auth == [5, 0], "SOCKS authentication failed");
    tcp.write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])?;
    let mut head = [0; 4];
    tcp.read_exact(&mut head)?;
    ensure!(head[..3] == [5, 0, 0], "SOCKS UDP association rejected");
    let relay_ip = match head[3] {
        1 => {
            let mut a = [0; 4];
            tcp.read_exact(&mut a)?;
            IpAddr::V4(a.into())
        }
        4 => {
            let mut a = [0; 16];
            tcp.read_exact(&mut a)?;
            IpAddr::V6(a.into())
        }
        _ => bail!("SOCKS relay must return an IP"),
    };
    let relay_ip = if relay_ip.is_unspecified() {
        if relay_ip.is_ipv4() {
            "127.0.0.1"
        } else {
            "::1"
        }
        .parse()?
    } else {
        relay_ip
    };
    let mut p = [0; 2];
    tcp.read_exact(&mut p)?;
    let relay = SocketAddr::new(relay_ip, u16::from_be_bytes(p));
    let udp = UdpSocket::bind(if relay.is_ipv4() {
        "127.0.0.1:0"
    } else {
        "[::1]:0"
    })?;
    udp.connect(relay)?;
    udp.set_read_timeout(Some(Duration::from_secs(10)))?;
    for sequence in 0..4u8 {
        let mut frame = vec![0, 0, 0];
        match target {
            IpAddr::V4(a) => {
                frame.push(1);
                frame.extend(a.octets());
            }
            IpAddr::V6(a) => {
                frame.push(4);
                frame.extend(a.octets());
            }
        }
        frame.extend(port.to_be_bytes());
        let payload = if mode == "dns" {
            let mut q = vec![0x51, sequence, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
            q.extend(b"\x07example\x03com\x00\x00\x01\x00\x01");
            q
        } else if mode == "stun" {
            let mut q = vec![0, 1, 0, 0, 0x21, 0x12, 0xa4, 0x42];
            q.extend(rand::random::<[u8; 12]>());
            q
        } else {
            (0..(256 + usize::from(sequence) * 257))
                .map(|n| (n as u8).wrapping_add(sequence))
                .collect::<Vec<_>>()
        };
        frame.extend(&payload);
        udp.send(&frame)?;
        let mut reply = [0u8; 65535];
        let n = udp.recv(&mut reply)?;
        ensure!(
            n >= 10 && reply[..3] == [0, 0, 0],
            "Invalid SOCKS UDP response"
        );
        let start = match reply[3] {
            1 => 10,
            4 => 22,
            3 => 7 + usize::from(reply[4]),
            _ => bail!("Invalid SOCKS address"),
        };
        ensure!(n >= start, "Truncated UDP frame");
        let data = &reply[start..n];
        if mode == "dns" {
            ensure!(
                data.len() >= 12
                    && data[..2] == payload[..2]
                    && data[2] & 0x80 != 0
                    && data[3] & 15 == 0
                    && data[7] > 0,
                "Invalid DNS reply"
            );
        } else if mode == "stun" {
            ensure!(
                data.len() >= 28
                    && data[..2] == [1, 1]
                    && data[4..20] == payload[4..20]
                    && data.len() == 20 + usize::from(u16::from_be_bytes([data[2], data[3]])),
                "Invalid STUN binding reply"
            );
            let mut attrs = &data[20..];
            let mut mapped = false;
            while attrs.len() >= 4 {
                let kind = u16::from_be_bytes([attrs[0], attrs[1]]);
                let n = usize::from(u16::from_be_bytes([attrs[2], attrs[3]]));
                ensure!(
                    attrs.len() >= 4 + n.div_ceil(4) * 4,
                    "Truncated STUN attribute"
                );
                if [1, 0x20].contains(&kind) && n >= 8 {
                    mapped = true;
                }
                attrs = &attrs[4 + n.div_ceil(4) * 4..];
            }
            ensure!(mapped && attrs.is_empty(), "STUN mapped address missing");
        } else {
            ensure!(data == payload, "UDP echo payload changed");
        }
    }
    println!("UDP_{}_FOUR_ROUND_TRIPS_PASS", mode.to_ascii_uppercase());
    Ok(())
}
