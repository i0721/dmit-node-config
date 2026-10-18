//! Minimal bounded HTTP transport on a private Unix socket; no public listener.
use crate::{
    share,
    store::{self, Store},
};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
};

const LIMIT: usize = 8192;
pub const SOCKET: &str = "/run/nodectl-share/api.sock";
pub fn health() -> Result<()> {
    #[cfg(unix)]
    {
        use std::{
            os::unix::net::UnixStream,
            time::{Duration, Instant},
        };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let probe = (|| -> Result<()> {
                let mut stream = UnixStream::connect(SOCKET)?;
                stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                stream.write_all(b"GET /nodes/ HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
                let mut buffer = [0u8; 32];
                let length = stream.read(&mut buffer)?;
                ensure!(
                    buffer[..length].starts_with(b"HTTP/1.1 200 OK"),
                    "Distribution health check failed"
                );
                Ok(())
            })();
            if probe.is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        anyhow::bail!("Distribution service did not become ready")
    }
    #[cfg(not(unix))]
    anyhow::bail!("Distribution requires Linux")
}
pub fn nginx_route() -> String {
    format!(
        r#"location ^~ /nodes/ {{
            access_log off;
            error_log /dev/null;
            limit_req zone=node_distribution burst=10 nodelay;
            limit_req_status 429;
            limit_conn node_distribution_connections 4;
            limit_conn_status 429;
            client_max_body_size 1k;
            proxy_pass http://unix:{SOCKET}:;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $remote_addr;
            proxy_set_header X-Forwarded-Proto https;
            proxy_http_version 1.1;
            proxy_set_header Connection close;
            proxy_cache off;
            proxy_buffering off;
            proxy_max_temp_file_size 0;
            proxy_connect_timeout 2s;
            proxy_read_timeout 5s;
            add_header Cache-Control "private, no-store, max-age=0" always;
            add_header Referrer-Policy no-referrer always;
            add_header X-Content-Type-Options nosniff always;
        }}"#
    )
}
const HEADERS: &str = "Cache-Control: private, no-store, max-age=0\r\nPragma: no-cache\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\nConnection: close\r\n";

fn response(stream: &mut impl Write, status: u16, kind: &str, body: &[u8]) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let extra = match status {
        401 => "WWW-Authenticate: Bearer realm=\"nodes\"\r\n",
        405 => "Allow: GET\r\n",
        429 | 503 => "Retry-After: 2\r\n",
        _ => "",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\n{HEADERS}{extra}Content-Type: {kind}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn error(stream: &mut impl Write, status: u16) -> Result<()> {
    response(
        stream,
        status,
        "text/plain; charset=utf-8",
        b"Request unavailable.\n",
    )
}

fn header<'a>(request: &'a httparse::Request<'_, '_>, name: &str) -> Result<Option<&'a str>> {
    let values = request
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name))
        .collect::<Vec<_>>();
    ensure!(values.len() <= 1, "Duplicate header");
    values
        .first()
        .map(|h| std::str::from_utf8(h.value).context("Invalid header"))
        .transpose()
}

fn route(
    store: &Store,
    request: &httparse::Request<'_, '_>,
    stream: &mut impl Write,
) -> Result<()> {
    if request.method != Some("GET") {
        return error(stream, 405);
    }
    if header(request, "transfer-encoding")?.is_some()
        || header(request, "content-length")?.is_some_and(|v| v != "0")
    {
        return error(stream, 400);
    }
    let target = request.path.context("Missing path")?;
    match target {
        "/nodes/" => {
            return response(
                stream,
                200,
                "text/html; charset=utf-8",
                include_bytes!("../assets/share/index.html"),
            );
        }
        "/nodes/app.js" => {
            return response(
                stream,
                200,
                "application/javascript; charset=utf-8",
                include_bytes!("../assets/share/app.js"),
            );
        }
        "/nodes/app.css" => {
            return response(
                stream,
                200,
                "text/css; charset=utf-8",
                include_bytes!("../assets/share/app.css"),
            );
        }
        _ => {}
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if !["/nodes/api", "/nodes/sub/xray", "/nodes/sub/mihomo"].contains(&path) {
        return error(stream, 404);
    }
    let bearer = header(request, "authorization")?.and_then(|v| v.strip_prefix("Bearer "));
    let query_token = query.strip_prefix("token=").filter(|v| !v.contains('&'));
    if (!query.is_empty() && query_token.is_none())
        || (bearer.is_some() && query_token.is_some())
        || (path == "/nodes/api" && !query.is_empty())
    {
        return error(stream, 400);
    }
    let token = bearer.or(query_token).unwrap_or("");
    // Shared advisory lock spans authentication, verification and the response;
    // rotation and deployment exclusively serialize against all new requests.
    let lock_path = store.state_dir.join("lock");
    if store::check_private(&lock_path, false).is_err() {
        return error(stream, 503);
    }
    let lock = fs::File::open(lock_path)?;
    if fs2::FileExt::try_lock_shared(&lock).is_err() {
        return error(stream, 503);
    }
    if !share::authorized(store, token) {
        return error(stream, 401);
    }
    let format = if path.ends_with("mihomo") {
        "mihomo"
    } else {
        "xray"
    };
    let Ok((id, bytes)) = share::current_clients(store, format) else {
        return error(stream, 503);
    };
    if path == "/nodes/api" {
        let profiles: serde_json::Value = serde_json::from_slice(&bytes)?;
        let body = serde_json::to_vec(&json!({"release":id,"clients":profiles,
            "subscriptions":{"xray":format!("/nodes/sub/xray?token={token}"),"mihomo":format!("/nodes/sub/mihomo?token={token}")}}))?;
        response(stream, 200, "application/json; charset=utf-8", &body)
    } else {
        // JSON is also a valid YAML provider and preserves renderer bytes.
        response(stream, 200, "application/json; charset=utf-8", &bytes)
    }
}

pub(crate) fn connection(store: &Store, stream: &mut (impl Read + Write)) -> Result<()> {
    let mut bytes = Vec::with_capacity(LIMIT);
    loop {
        let mut buffer = [0u8; 1024];
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > LIMIT {
            return error(stream, 400);
        }
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&bytes) {
            Ok(httparse::Status::Partial) => continue,
            Ok(httparse::Status::Complete(length)) if length == bytes.len() => {
                // Suppress internal details on every response/error path.
                if route(store, &request, stream).is_err() {
                    return error(stream, 400);
                }
                return Ok(());
            }
            _ => return error(stream, 400),
        }
    }
}

pub fn serve(store: &Store) -> Result<()> {
    #[cfg(unix)]
    {
        use std::{
            os::unix::{
                fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
                net::UnixListener,
            },
            sync::Arc,
            time::Duration,
        };
        crate::host::linux_root()?;
        let (c, _) = store.inputs()?;
        ensure!(c.distributes(), "Distribution is disabled");
        let name = std::ffi::CString::new(c.runtime.nginx_user)?;
        let user = unsafe { libc::getpwnam(name.as_ptr()) };
        ensure!(!user.is_null(), "Nginx account is unavailable");
        let gid = unsafe { (*user).pw_gid };
        let directory = std::path::Path::new(SOCKET).parent().unwrap();
        if !directory.exists() {
            fs::DirBuilder::new().mode(0o750).create(directory)?;
        }
        let metadata = fs::symlink_metadata(directory)?;
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == 0
                && metadata.mode() & 0o007 == 0,
            "Unsafe distribution socket directory"
        );
        let dirname = std::ffi::CString::new(directory.as_os_str().as_encoded_bytes())?;
        ensure!(
            unsafe { libc::chown(dirname.as_ptr(), 0, gid) } == 0,
            "Cannot protect socket directory"
        );
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750))?;
        if let Ok(metadata) = fs::symlink_metadata(SOCKET) {
            ensure!(
                metadata.file_type().is_socket() && metadata.uid() == 0,
                "Unsafe existing distribution socket"
            );
            fs::remove_file(SOCKET)?;
        }
        let listener = Arc::new(UnixListener::bind(SOCKET)?);
        let socket_name = std::ffi::CString::new(SOCKET)?;
        ensure!(
            unsafe { libc::chown(socket_name.as_ptr(), 0, gid) } == 0,
            "Cannot protect socket"
        );
        fs::set_permissions(SOCKET, fs::Permissions::from_mode(0o660))?;
        let mut workers = vec![];
        for _ in 0..8 {
            let listener = listener.clone();
            let store = Store::new(store.config_dir.clone(), store.state_dir.clone())?;
            workers.push(std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    if stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .is_err()
                        || stream
                            .set_write_timeout(Some(Duration::from_secs(3)))
                            .is_err()
                    {
                        continue;
                    }
                    let _ = connection(&store, &mut stream);
                }
            }));
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("Distribution worker stopped"))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = store;
        anyhow::bail!("Distribution requires Linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Exchange {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Exchange {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(bytes)
        }
    }
    impl Write for Exchange {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.output.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn request(store: &Store, request: &str) -> String {
        let mut exchange = Exchange {
            input: Cursor::new(request.as_bytes().to_vec()),
            output: vec![],
        };
        connection(store, &mut exchange).unwrap();
        String::from_utf8(exchange.output).unwrap()
    }
    fn get(store: &Store, path: &str, token: Option<&str>) -> String {
        request(
            store,
            &format!(
                "GET {path} HTTP/1.1\r\nHost: example.com\r\n{}\r\n",
                token
                    .map(|t| format!("Authorization: Bearer {t}\r\n"))
                    .unwrap_or_default()
            ),
        )
    }
    fn token(store: &Store) -> String {
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(share::credential_path(store)).unwrap()).unwrap();
        record["token"].as_str().unwrap().to_string()
    }
    fn deployed(store: &Store) -> String {
        let (mut c, s) = store.inputs().unwrap();
        c.distribution = Some(crate::config::Distribution { enabled: true });
        store::atomic_write(
            &store.config_dir.join("node.toml"),
            toml::to_string_pretty(&c).unwrap().as_bytes(),
        )
        .unwrap();
        let _ = s;
        let id = store.render().unwrap();
        store::private_dir(&store.state_dir.join("success")).unwrap();
        store::atomic_write(
            &store.state_dir.join("success").join(&id),
            &serde_json::to_vec(&json!({"release":id})).unwrap(),
        )
        .unwrap();
        store.set_pointer("current", Some(&id)).unwrap();
        id
    }
    #[test]
    fn authenticated_formats_follow_success_and_rotation_without_server_secrets() {
        let (_tmp, store) = crate::tests::sandbox();
        drop(store.lock().unwrap());
        share::rotate(&store, None).unwrap();
        let first = token(&store);
        assert!(get(&store, "/nodes/", None).starts_with("HTTP/1.1 200"));
        assert!(get(&store, "/nodes/api", None).starts_with("HTTP/1.1 401"));
        assert!(get(&store, "/nodes/api", Some(&first)).starts_with("HTTP/1.1 503"));
        let id = deployed(&store);
        for format in ["xray", "mihomo"] {
            let response = get(&store, &format!("/nodes/sub/{format}?token={first}"), None);
            assert!(response.starts_with("HTTP/1.1 200"));
            let body = response.split_once("\r\n\r\n").unwrap().1;
            assert_eq!(
                body.as_bytes(),
                share::current_clients(&store, format).unwrap().1
            );
            let (_, secrets) = store.inputs().unwrap();
            assert!(!body.contains(&secrets.reality_private_key));
            assert!(!body.contains("privateKey"));
            assert!(!body.contains("decryption"));
        }
        assert!(get(&store, "/nodes/api", Some(&first)).contains(&id));
        share::rotate(&store, None).unwrap();
        let second = token(&store);
        assert!(get(&store, "/nodes/api", Some(&first)).starts_with("HTTP/1.1 401"));
        assert!(get(&store, "/nodes/api", Some(&second)).starts_with("HTTP/1.1 200"));
        share::revoke(&store).unwrap();
        assert!(get(&store, "/nodes/api", Some(&second)).starts_with("HTTP/1.1 401"));
        // Neither pointer rollback nor a legacy successful snapshot restores access.
        store.set_pointer("current", Some(&id)).unwrap();
        assert!(get(&store, "/nodes/api", Some(&second)).starts_with("HTTP/1.1 401"));
    }
    #[test]
    fn invalid_requests_are_generic_uncached_and_never_echo_credentials() {
        let (_tmp, store) = crate::tests::sandbox();
        drop(store.lock().unwrap());
        share::rotate(&store, None).unwrap();
        let secret = token(&store);
        for raw in [
            format!(
                "GET /nodes/api HTTP/1.1\r\nAuthorization: Bearer {secret}\r\nAuthorization: Bearer {secret}\r\n\r\n"
            ),
            format!("GET /nodes/sub/xray?token={secret}&token={secret} HTTP/1.1\r\n\r\n"),
            format!(
                "GET /nodes/sub/xray?token={secret} HTTP/1.1\r\nAuthorization: Bearer {secret}\r\n\r\n"
            ),
            format!(
                "GET /nodes/api HTTP/1.1\r\nTransfer-Encoding: chunked\r\nAuthorization: Bearer {secret}\r\n\r\n"
            ),
            format!(
                "GET /nodes/api HTTP/1.1\r\nAuthorization: Bearer {secret}\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n"
            ),
            format!(
                "GET /nodes/api HTTP/1.1\r\nAuthorization: Bearer {secret}\r\nExtra: {}\r\n\r\n",
                "x".repeat(9000)
            ),
        ] {
            let response = request(&store, &raw);
            assert!(response.starts_with("HTTP/1.1 400"));
            assert!(response.contains("Cache-Control: private, no-store"));
            assert!(response.contains("Referrer-Policy: no-referrer"));
            assert!(!response.contains(&secret));
            assert!(!response.contains("Access-Control-Allow-Origin"));
        }
    }
    #[test]
    fn exclusive_mutations_and_bad_success_records_fail_closed() {
        let (_tmp, store) = crate::tests::sandbox();
        share::rotate(&store, None).unwrap();
        let secret = token(&store);
        let id = deployed(&store);
        {
            let _lock = store.lock().unwrap();
            assert!(get(&store, "/nodes/api", Some(&secret)).starts_with("HTTP/1.1 503"));
        }
        store::atomic_write(
            &store.state_dir.join("success").join(&id),
            b"{\"release\":\"wrong\"}",
        )
        .unwrap();
        assert!(get(&store, "/nodes/api", Some(&secret)).starts_with("HTTP/1.1 503"));
    }
}
