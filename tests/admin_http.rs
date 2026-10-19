//! Real HTTP/CLI contract test. Uses an isolated owner-only store, no host services.
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn cli(config: &std::path::Path, state: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nodectl"))
        .arg("--config-dir")
        .arg(config)
        .arg("--state-dir")
        .arg(state)
        .args(args)
        .output()
        .unwrap()
}
struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    data: Option<&Value>,
) -> (u16, String, Value) {
    let bytes = data
        .map(|v| serde_json::to_vec(v).unwrap())
        .unwrap_or_default();
    let mut stream =
        TcpStream::connect_timeout(&"127.0.0.1:9080".parse().unwrap(), Duration::from_secs(2))
            .unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n",bytes.len()).unwrap();
    for (k, v) in headers {
        write!(stream, "{k}: {v}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(&bytes).unwrap();
    let mut response = vec![];
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
    let head = String::from_utf8(response[..split].to_vec()).unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = serde_json::from_slice(&response[split + 4..]).unwrap_or(Value::Null);
    (status, head, body)
}
#[test]
fn real_api_preserves_inputs_and_enforces_session_policy_review_and_reset() {
    assert!(
        TcpStream::connect_timeout(
            &"127.0.0.1:9080".parse().unwrap(),
            Duration::from_millis(200)
        )
        .is_err(),
        "Stop the local development API before running HTTP regression"
    );
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let state = tmp.path().join("state");
    let mut example: toml::Value =
        toml::from_str(include_str!("../config/node.example.toml")).unwrap();
    example.as_table_mut().unwrap().remove("vless");
    fs::create_dir(&config).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Imported valid identities exercise preservation without needing Xray
    // installed on the developer host. No production config is read.
    let fixture = json!({"xhttp_uuid":"10a6cfe1-d381-4c46-865e-998b28b801a1","ws_uuid":"20a6cfe1-d381-4c46-865e-998b28b801a2","reality_uuid":"30a6cfe1-d381-4c46-865e-998b28b801a3","reality_private_key":"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc","reality_short_id":"0123456789abcdef"});
    let mut secret_file = tempfile::NamedTempFile::new_in(&config).unwrap();
    secret_file
        .write_all(&serde_json::to_vec(&fixture).unwrap())
        .unwrap();
    secret_file.persist(config.join("secrets.json")).unwrap();
    let input = tmp.path().join("example.toml");
    fs::write(&input, toml::to_string(&example).unwrap()).unwrap();
    let init = cli(
        &config,
        &state,
        &["init", "--example", input.to_str().unwrap()],
    );
    assert!(init.status.success());
    let original = fs::read(config.join("node.toml")).unwrap();
    let secret = fs::read(config.join("secrets.json")).unwrap();
    let token = cli(&config, &state, &["admin", "init"]);
    assert!(token.status.success());
    let token = String::from_utf8(token.stdout)
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_owned();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_nodectl"))
            .arg("--config-dir")
            .arg(&config)
            .arg("--state-dir")
            .arg(&state)
            .args([
                "admin",
                "serve",
                "--origin",
                "http://127.0.0.1:4173",
                "--development",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        if TcpStream::connect_timeout(
            &"127.0.0.1:9080".parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "API exited before listening"
        );
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(30));
    }
    let (status, head, _) = request("GET", "/admin/api/config", &[], None);
    assert_eq!(status, 401);
    assert!(
        head.to_lowercase()
            .contains("cache-control: private, no-store")
    );
    let headers = vec![
        ("Content-Type", "application/json".into()),
        ("Origin", "http://127.0.0.1:4173".into()),
    ];
    assert_eq!(
        request(
            "POST",
            "/admin/api/login",
            &headers[..1],
            Some(&json!({"token":token}))
        )
        .0,
        403
    );
    let (status, head, login) = request(
        "POST",
        "/admin/api/login",
        &headers,
        Some(&json!({"token":token})),
    );
    assert_eq!(status, 200);
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut authenticated = headers.clone();
    authenticated.push(("Cookie", cookie));
    authenticated.push(("X-CSRF-Token", login["csrf"].as_str().unwrap().into()));
    let (status, _, view) = request("GET", "/admin/api/config", &authenticated, None);
    assert_eq!(status, 200);
    assert!(view.get("secrets").is_none());
    let mut candidate = view["config"].clone();
    candidate["clients"]["xhttp_weight"] = json!(75);
    candidate["clients"]["ws_weight"] = json!(20);
    assert_eq!(
        request(
            "POST",
            "/admin/api/config/preview",
            &authenticated,
            Some(&json!({"config":candidate}))
        )
        .0,
        403
    );
    let (status, _, unlock) = request(
        "POST",
        "/admin/api/unlock",
        &authenticated,
        Some(&json!({"token":token})),
    );
    assert_eq!(status, 200);
    let mut socket_candidate = view["config"].clone();
    socket_candidate["transport"]["xhttp"]["socket"] = json!("/etc/forbidden-admin.sock");
    let (status, _, denied) = request(
        "POST",
        "/admin/api/config/preview",
        &authenticated,
        Some(&json!({"config":socket_candidate,"unlock":unlock["unlock"]})),
    );
    assert_eq!(status, 403);
    assert_eq!(denied["error"], "immutable_platform_boundary");
    candidate = view["config"].clone();
    candidate["node"]["ipv4"] = json!("203.0.113.11");
    let (status, _, plan) = request(
        "POST",
        "/admin/api/config/preview",
        &authenticated,
        Some(&json!({"config":candidate})),
    );
    assert_eq!(status, 200);
    let review = json!({"plan":plan["plan"],"confirm":plan["plan"]});
    assert_eq!(
        request(
            "POST",
            "/admin/api/config/apply",
            &authenticated,
            Some(&review)
        )
        .0,
        409
    );
    assert_eq!(
        request(
            "POST",
            "/admin/api/config/apply",
            &authenticated[..3],
            Some(&review)
        )
        .0,
        403
    );
    assert_eq!(
        request(
            "GET",
            "/admin/api/config?token=forbidden",
            &authenticated,
            None
        )
        .0,
        400
    );
    assert_eq!(fs::read(config.join("node.toml")).unwrap(), original);
    assert_eq!(fs::read(config.join("secrets.json")).unwrap(), secret);
    assert!(
        cli(&config, &state, &["admin", "reset-token"])
            .status
            .success()
    );
    assert_eq!(
        request("GET", "/admin/api/config", &authenticated, None).0,
        401
    );
    let mut limited = false;
    for _ in 0..12 {
        let (status, _, _) = request(
            "POST",
            "/admin/api/login",
            &headers,
            Some(&json!({"token":"bad"})),
        );
        assert!(status == 401 || status == 429);
        limited |= status == 429;
    }
    assert!(limited);
    let audit = fs::read_to_string(state.join("admin/audit.jsonl")).unwrap();
    assert!(!audit.contains(&token));
}
