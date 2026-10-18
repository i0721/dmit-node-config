//! Explicit native upstream tests; no containers and no external accounts.
use crate::{host, store};
use rand::RngCore;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Process(Child);

#[test]
#[ignore = "writes a routing fixture only when explicitly requested by the namespace driver"]
fn generate_cn_forwarding_fixture_from_production_model() {
    let destination =
        std::env::var("CN_ROUTING_TEST_CONFIG").expect("Explicit fixture destination");
    let mut c: crate::config::Config = toml::from_str(crate::config::EXAMPLE).unwrap();
    c.node.ipv6.clear(); // Keep the DNS responder A-only; literal IPv6 is also tested.
    c.cn_warp = Some(crate::config::CnWarp::managed(true));
    let mut configuration = json!({"log":{"loglevel":"none"},
        "inbounds":[{"listen":"127.0.0.1","port":10810,"protocol":"socks","settings":{"auth":"noauth","udp":true}}],
        "outbounds":[{"tag":"direct","protocol":"freedom"},{"tag":"block","protocol":"blackhole"}]});
    crate::cn_warp::xray(&c, &mut configuration);
    fs::write(
        destination,
        serde_json::to_vec_pretty(&configuration).unwrap(),
    )
    .unwrap();
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
fn random() -> String {
    let mut value = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut value);
    hex::encode(value)
}

fn http(
    url: &str,
    method: &str,
    token: Option<&str>,
    body: Option<&Path>,
    headers: &[(&str, &str)],
    output: Option<&Path>,
) -> Vec<u8> {
    let mut configuration = format!(
        "url = {}\nrequest = {}\nsilent\nshow-error\nmax-time = 35\nnoproxy = \"*\"\n",
        json!(url),
        json!(method)
    );
    if let Some(token) = token {
        configuration.push_str(&format!(
            "header = {}\n",
            json!(format!("Authorization: {token}"))
        ));
    }
    for (name, value) in headers {
        configuration.push_str(&format!("header = {}\n", json!(format!("{name}: {value}"))));
    }
    if let Some(body) = body {
        configuration.push_str(&format!(
            "data-binary = {}\n",
            json!(format!("@{}", body.display()))
        ));
    }
    if let Some(output) = output {
        configuration.push_str(&format!("output = {}\n", json!(output.to_str().unwrap())));
    }
    host::run(
        "curl",
        &["--disable", "--config", "-"],
        Some(configuration.as_bytes()),
    )
    .unwrap()
}
fn api(
    base: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
    directory: &Path,
) -> Value {
    let file = directory.join("request.json");
    if let Some(body) = &body {
        store::atomic_write(&file, &serde_json::to_vec(body).unwrap()).unwrap();
    }
    serde_json::from_slice(&http(
        &format!("{base}{path}"),
        if body.is_some() { "POST" } else { "GET" },
        token,
        body.as_ref().map(|_| file.as_path()),
        &[("Content-Type", "application/json")],
        None,
    ))
    .unwrap()
}
fn spawn(binary: &str, data: &Path, password: &str) -> Process {
    let diagnostic = std::env::var_os("OPENLIST_TEST_DIAGNOSTIC").map(|p| {
        store::atomic_write(Path::new(&p), b"").unwrap();
        fs::OpenOptions::new().write(true).open(p).unwrap()
    });
    Process(
        Command::new(binary)
            .args(["server", "--data", data.to_str().unwrap()])
            .env("OPENLIST_ADMIN_PASSWORD", password)
            .stdout(Stdio::null())
            .stderr(diagnostic.map(Stdio::from).unwrap_or_else(Stdio::null))
            .spawn()
            .unwrap(),
    )
}
fn ready(base: &str) {
    let started = Instant::now();
    while host::command(
        "curl",
        &[
            "--silent",
            "--fail",
            "--noproxy",
            "*",
            "--max-time",
            "1",
            &format!("{base}/api/public/settings"),
        ],
    )
    .is_err()
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "Native OpenList did not become ready"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "Requires locked LIBRESPEED_TEST_BINARY and OPENLIST_TEST_BINARY; native loopback processes"]
fn native_librespeed_and_openlist_real_transfers_permissions_and_restart() {
    let speed = std::env::var("LIBRESPEED_TEST_BINARY").unwrap();
    let files = std::env::var("OPENLIST_TEST_BINARY").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let storage = tmp.path().join("storage");
    store::private_dir(&data).unwrap();
    store::private_dir(&storage).unwrap();
    let speed_port = port();
    let files_port = port();
    assert_ne!(speed_port, files_port);
    let settings = tmp.path().join("settings.toml");
    store::atomic_write(&settings,format!("bind_address=\"127.0.0.1\"\nlisten_port={speed_port}\nurl_base=\"/librespeed\"\nproxyprotocol_port=0\nserver_lat=1\nserver_lng=1\ndatabase_type=\"none\"\nstatistics_password=\"\"\n").as_bytes()).unwrap();
    let _speed = Process(
        Command::new(speed)
            .args(["-c", settings.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let speed_base = format!("http://127.0.0.1:{speed_port}/librespeed");
    let started = Instant::now();
    while host::command(
        "curl",
        &[
            "--silent",
            "--fail",
            "--noproxy",
            "*",
            "--max-time",
            "1",
            &format!("{speed_base}/empty.php"),
        ],
    )
    .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(100));
    }
    let download = tmp.path().join("download.bin");
    http(
        &format!("{speed_base}/garbage.php?ckSize=1"),
        "GET",
        None,
        None,
        &[],
        Some(&download),
    );
    assert!(fs::metadata(&download).unwrap().len() >= 1024 * 1024);
    http(
        &format!("{speed_base}/empty.php"),
        "POST",
        None,
        Some(&download),
        &[],
        None,
    );
    println!("OFFICIAL_LIBRESPEED_NATIVE_REAL_UPLOAD_DOWNLOAD_PASS");
    store::atomic_write(&data.join("config.json"),&serde_json::to_vec(&json!({
        "scheme":{"address":"127.0.0.1","http_port":files_port,"https_port":-1,"enable_h3":false},
        "database":{"type":"sqlite3","db_file":data.join("data.db"),"table_prefix":"test_"},
        "log":{"enable":false},"temp_dir":data.join("temp")
    })).unwrap()).unwrap();
    let password = random();
    let mut process = spawn(&files, &data, &password);
    let base = format!("http://127.0.0.1:{files_port}");
    ready(&base);
    let login = api(
        &base,
        "/api/auth/login",
        None,
        Some(json!({"username":"admin","password":password})),
        tmp.path(),
    );
    assert!(login["code"] == 200, "OpenList first administrator failed");
    let token = login["data"]["token"].as_str().unwrap();
    let create = api(
        &base,
        "/api/admin/storage/create",
        Some(token),
        Some(json!({
            "mount_path":"/","driver":"Local","addition":json!({"root_folder_path":storage}).to_string(),
            "web_proxy":true,"enable_sign":true,"cache_expiration":0
        })),
        tmp.path(),
    );
    assert!(create["code"] == 200, "Local storage provisioning failed");
    let upload = tmp.path().join("upload.bin");
    let mut payload = fs::File::create(&upload).unwrap();
    let mut block = vec![0; 1024 * 1024];
    rand::rngs::OsRng.fill_bytes(&mut block);
    for _ in 0..64 {
        payload.write_all(&block).unwrap();
    }
    drop(payload);
    let response: Value = serde_json::from_slice(&http(
        &format!("{base}/api/fs/put"),
        "PUT",
        Some(token),
        Some(&upload),
        &[
            ("File-Path", "/large.bin"),
            ("Content-Type", "application/octet-stream"),
        ],
        None,
    ))
    .unwrap();
    assert!(response["code"] == 200, "Admin upload failed");
    assert_eq!(
        store::file_digest(&upload).unwrap(),
        store::file_digest(&storage.join("large.bin")).unwrap()
    );
    let get = api(
        &base,
        "/api/fs/get",
        Some(token),
        Some(json!({"path":"/large.bin","password":""})),
        tmp.path(),
    );
    assert!(get["code"] == 200, "Download metadata unavailable");
    let url = get["data"]["raw_url"].as_str().unwrap();
    http(url, "GET", None, None, &[], Some(&download));
    assert_eq!(
        store::file_digest(&upload).unwrap(),
        store::file_digest(&download).unwrap()
    );
    http(
        url,
        "GET",
        None,
        None,
        &[("Range", "bytes=1048576-2097151")],
        Some(&download),
    );
    assert!(
        fs::read(&download).unwrap() == block,
        "Range download differs from uploaded bytes"
    );
    let anonymous = api(
        &base,
        "/api/fs/get",
        None,
        Some(json!({"path":"/large.bin"})),
        tmp.path(),
    );
    assert!(
        anonymous["code"] != 200,
        "Anonymous file access unexpectedly permitted"
    );
    let reader_password = random();
    let user = api(
        &base,
        "/api/admin/user/create",
        Some(token),
        Some(
            json!({"username":"reader","password":reader_password,"base_path":"/","permission":0,"role":0}),
        ),
        tmp.path(),
    );
    assert!(user["code"] == 200, "Read-only user provisioning failed");
    let reader = api(
        &base,
        "/api/auth/login",
        None,
        Some(json!({"username":"reader","password":reader_password})),
        tmp.path(),
    );
    assert!(reader["code"] == 200);
    let reader = reader["data"]["token"].as_str().unwrap();
    let read = api(
        &base,
        "/api/fs/get",
        Some(reader),
        Some(json!({"path":"/large.bin"})),
        tmp.path(),
    );
    assert!(read["code"] == 200, "Read-only user cannot read");
    let denied: Value = serde_json::from_slice(&http(
        &format!("{base}/api/fs/put"),
        "PUT",
        Some(reader),
        Some(&download),
        &[("File-Path", "/forbidden.bin")],
        None,
    ))
    .unwrap();
    assert!(
        denied["code"] != 200 && !storage.join("forbidden.bin").exists(),
        "Read-only user obtained write permission"
    );
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    // A changed initial-password environment must never rotate an existing DB.
    process = spawn(&files, &data, &random());
    ready(&base);
    let preserved = api(
        &base,
        "/api/auth/login",
        None,
        Some(json!({"username":"admin","password":password})),
        tmp.path(),
    );
    assert!(
        preserved["code"] == 200,
        "Restart changed an existing administrator"
    );
    assert_eq!(
        fs::metadata(storage.join("large.bin")).unwrap().len(),
        64 * 1024 * 1024
    );
    drop(process);
    println!("OFFICIAL_OPENLIST_NATIVE_64M_UPLOAD_DOWNLOAD_RANGE_READONLY_ANONYMOUS_RESTART_PASS");
}

#[test]
#[ignore = "requires an explicitly selected disposable managed HTTPS fixture"]
fn managed_nginx_openlist_upload_download_sandbox_and_permissions() {
    let base = std::env::var("OPENLIST_MANAGED_TEST_URL").expect("Explicit managed fixture URL");
    assert!(base.starts_with("https://") && base.ends_with(".integration.test"));
    let password_file = Path::new(crate::web_services::INITIAL_PASSWORD);
    store::check_private(password_file, false).unwrap();
    let private = fs::read_to_string(password_file).unwrap();
    let password = private
        .trim()
        .strip_prefix("OPENLIST_ADMIN_PASSWORD=")
        .unwrap();
    let work = tempfile::tempdir().unwrap();
    let login = api(
        &base,
        "/api/auth/login",
        None,
        Some(json!({"username":"admin","password":password})),
        work.path(),
    );
    assert!(login["code"] == 200, "Managed administrator login failed");
    let token = login["data"]["token"].as_str().unwrap();
    let original = work.path().join("original.bin");
    let mut bytes = vec![0; 64 * 1024 * 1024];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    store::atomic_write(&original, &bytes).unwrap();
    let uploaded: Value = serde_json::from_slice(&http(
        &format!("{base}/api/fs/put"),
        "PUT",
        Some(token),
        Some(&original),
        &[("File-Path", "/acceptance-large.bin")],
        None,
    ))
    .unwrap();
    assert!(uploaded["code"] == 200, "Managed HTTPS upload failed");
    let item = api(
        &base,
        "/api/fs/get",
        Some(token),
        Some(json!({"path":"/acceptance-large.bin"})),
        work.path(),
    );
    assert!(item["code"] == 200, "Managed file lookup failed");
    let url = item["data"]["raw_url"].as_str().unwrap();
    assert!(
        url.starts_with(&format!("{base}/p/")),
        "Managed signed download did not use the protected HTTPS proxy"
    );
    let downloaded = work.path().join("downloaded.bin");
    http(url, "GET", None, None, &[], Some(&downloaded));
    assert_eq!(
        store::file_digest(&original).unwrap(),
        store::file_digest(&downloaded).unwrap()
    );
    let range = http(
        url,
        "GET",
        None,
        None,
        &[("Range", "bytes=1024-1049599")],
        None,
    );
    assert_eq!(range, bytes[1024..1049600]);
    let config = format!(
        "url = {}\nsilent\nshow-error\nhead\nnoproxy = \"*\"\nmax-time = 10\n",
        json!(url)
    );
    let headers = String::from_utf8(
        host::run(
            "curl",
            &["--disable", "--config", "-"],
            Some(config.as_bytes()),
        )
        .unwrap(),
    )
    .unwrap()
    .to_ascii_lowercase();
    assert!(
        headers.contains("content-security-policy: sandbox;")
            && headers.contains("x-content-type-options: nosniff")
            && headers.contains("content-disposition: attachment"),
        "Uploaded content lacks browser isolation headers"
    );
    let denied = api(
        &base,
        "/api/fs/get",
        None,
        Some(json!({"path":"/acceptance-large.bin"})),
        work.path(),
    );
    assert!(
        denied["code"] != 200,
        "Anonymous access unexpectedly allowed"
    );
    let reader_name = format!("reader-{}", &random()[..12]);
    let reader_password = random();
    let created = api(
        &base,
        "/api/admin/user/create",
        Some(token),
        Some(
            json!({"username":reader_name,"password":reader_password,"base_path":"/","permission":0,"role":0}),
        ),
        work.path(),
    );
    assert!(
        created["code"] == 200,
        "Managed read-only user creation failed"
    );
    let reader = api(
        &base,
        "/api/auth/login",
        None,
        Some(json!({"username":reader_name,"password":reader_password})),
        work.path(),
    );
    assert!(reader["code"] == 200, "Managed read-only login failed");
    let reader_token = reader["data"]["token"].as_str().unwrap();
    let read = api(
        &base,
        "/api/fs/get",
        Some(reader_token),
        Some(json!({"path":"/acceptance-large.bin"})),
        work.path(),
    );
    assert!(read["code"] == 200, "Managed read-only user could not read");
    let small = work.path().join("denied.txt");
    store::atomic_write(&small, b"must not be published").unwrap();
    let denied: Value = serde_json::from_slice(&http(
        &format!("{base}/api/fs/put"),
        "PUT",
        Some(reader_token),
        Some(&small),
        &[("File-Path", "/readonly-must-not-write.txt")],
        None,
    ))
    .unwrap();
    assert!(denied["code"] != 200, "Managed read-only user could write");
    println!(
        "MANAGED_NGINX_OPENLIST_HTTPS_64M_UPLOAD_DOWNLOAD_RANGE_SANDBOX_ANONYMOUS_READONLY_PASS"
    );
}
