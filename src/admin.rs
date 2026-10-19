//! Bounded single-admin API. No shell, user accounts, database or public listener.
use crate::{
    admin_auth as auth, admin_policy as policy, admin_read as read,
    config::{self, Config},
    host,
    store::{self, Store},
};
use anyhow::{Context, Result, bail, ensure};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;

#[derive(Subcommand)]
pub enum Action {
    /// Preview/install static assets and a separate Nginx HTTPS listener on 9443.
    Install {
        #[arg(long, required_unless_present = "archive", conflicts_with = "archive")]
        assets: Option<PathBuf>,
        /// Local release archive, verified by install.sh before installation.
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        execute: bool,
        #[arg(long,default_value_t=9443,value_parser=clap::value_parser!(u16).range(1024..))]
        port: u16,
    },
    /// Generate a credential once; preserve an existing administrator.
    Init,
    /// Invalidate all sessions and print a new administrator token once.
    ResetToken,
    /// Serve a loopback API behind the dedicated Nginx HTTPS endpoint.
    Serve {
        #[arg(long)]
        origin: String,
        /// Explicit local development only; cookies are not Secure on loopback HTTP.
        #[arg(long)]
        development: bool,
    },
}
pub fn run(store: Store, action: Action) -> Result<()> {
    match action {
        Action::Install {
            assets,
            archive,
            execute,
            port,
        } => {
            if let Some(archive) = archive {
                crate::admin_install::from_archive(&store, &archive, port, execute)?;
            } else {
                crate::admin_install::run(
                    &store,
                    &assets.context("Static assets required")?,
                    port,
                    execute,
                )?;
            }
        }
        Action::Init | Action::ResetToken => {
            let force = matches!(action, Action::ResetToken);
            if let Some(token) = auth::reset(&store, force)? {
                println!("Administrator token (save securely; shown once):\n{token}");
            } else {
                println!(
                    "Administrator credential preserved. Use nodectl admin reset-token to rotate."
                );
            }
        }
        Action::Serve {
            origin,
            development,
        } => {
            valid_origin(&origin, development)?;
            auth::generation(&store).context("Run nodectl admin init first")?;
            let jobs = load_jobs(&store)?;
            let app = Arc::new(App {
                store,
                origin,
                development,
                auth: Mutex::new(auth::Auth::default()),
                meter: Mutex::new(read::Meter::default()),
                plans: Mutex::new(BTreeMap::new()),
                jobs: Mutex::new(jobs),
                busy: AtomicBool::new(false),
                audit_lock: Mutex::new(()),
                requests: Arc::new(tokio::sync::Semaphore::new(16)),
            });
            tokio::runtime::Runtime::new()?.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:9080").await?;
                println!("Management API listening on 127.0.0.1:9080");
                axum::serve(listener, router(app))
                    .with_graceful_shutdown(async {
                        let _ = tokio::signal::ctrl_c().await;
                    })
                    .await?;
                Ok::<_, anyhow::Error>(())
            })?;
        }
    }
    Ok(())
}
fn valid_origin(origin: &str, development: bool) -> Result<()> {
    let u: Uri = origin.parse()?;
    ensure!(
        u.authority().is_some()
            && !origin.contains('@')
            && u.path() == "/"
            && u.query().is_none()
            && !origin.ends_with('/'),
        "invalid_origin"
    );
    if development {
        ensure!(
            u.scheme_str() == Some("http")
                && matches!(u.host(), Some("127.0.0.1") | Some("localhost")),
            "development_requires_loopback"
        );
    } else {
        ensure!(u.scheme_str() == Some("https"), "https_required");
    }
    Ok(())
}
#[derive(Clone)]
struct Plan {
    id: String,
    release: String,
    owner: String,
    base: String,
    expires: u64,
    candidate: Config,
    advanced: bool,
    checked: bool,
    changes: Vec<Value>,
    diff: String,
    generation: String,
    deployment: Option<Deployment>,
    restore_secrets: Option<config::Secrets>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    files: BTreeMap<String, String>,
    current: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    backup: String,
    token: String,
}
#[derive(Clone)]
struct Deployment {
    options: ProvisionOptions,
    digest: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionOptions {
    manual_certificates: bool,
    #[serde(default)]
    acme_email: Option<String>,
    #[serde(default)]
    accept_ca_terms: bool,
    #[serde(default)]
    accept_warp_terms: bool,
    #[serde(default)]
    skip_dependencies: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionApproval {
    plan: String,
    options: ProvisionOptions,
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    id: String,
    operation: String,
    phase: String,
    started: u64,
    finished: Option<u64>,
    ok: Option<bool>,
    result: Value,
}
struct App {
    store: Store,
    origin: String,
    development: bool,
    auth: Mutex<auth::Auth>,
    meter: Mutex<read::Meter>,
    plans: Mutex<BTreeMap<String, Plan>>,
    jobs: Mutex<BTreeMap<String, Job>>,
    busy: AtomicBool,
    audit_lock: Mutex<()>,
    requests: Arc<tokio::sync::Semaphore>,
}
fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/admin/api/{*path}", any(handle))
        .layer(DefaultBodyLimit::max(128 * 1024))
        .with_state(app)
}
fn load_jobs(store: &Store) -> Result<BTreeMap<String, Job>> {
    let p = auth::directory(store).join("jobs.json");
    if !p.exists() {
        return Ok(BTreeMap::new());
    }
    store::check_private(&p, false)?;
    ensure!(
        fs::metadata(&p)?.len() < 2 * 1024 * 1024,
        "job_history_too_large"
    );
    let values: Vec<Job> = serde_json::from_slice(&fs::read(p)?)?;
    Ok(values
        .into_iter()
        .take(100)
        .map(|mut job| {
            if job.finished.is_none() {
                job.finished = Some(auth::now());
                job.ok = Some(false);
                job.phase = "interrupted".into();
                job.result = json!({"message":"API 进程已重启；请检查事务和恢复状态。"});
            }
            (job.id.clone(), job)
        })
        .collect())
}
fn redact_log(store: &Store, mut text: String) -> Result<String> {
    let (_, secrets) = store.inputs()?;
    let mut sensitive = secrets
        .sensitive()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(previous) = store.pointer("previous")? {
        let (_, secrets) = store.verify(&previous)?;
        sensitive.extend(secrets.sensitive().into_iter().map(str::to_owned));
    }
    let token = store.config_dir.join("credentials/cloudflare-token");
    if token.exists() {
        store::check_private(&token, false)?;
        ensure!(fs::metadata(&token)?.len() <= 4096, "Credential size");
        sensitive.push(fs::read_to_string(token)?.trim().into());
    }
    for secret in sensitive {
        if !secret.is_empty() {
            text = text.replace(&secret, "<redacted>");
        }
    }
    Ok(regex::Regex::new(r"(?i)[a-z0-9_+/=-]{32,}")?
        .replace_all(&text, "<redacted>")
        .into_owned())
}
fn text<'a>(h: &'a HeaderMap, name: &str) -> Result<Option<&'a str>> {
    ensure!(h.get_all(name).iter().count() <= 1, "bad_request");
    h.get(name)
        .map(|v| v.to_str().map_err(Into::into))
        .transpose()
}
fn cookie(h: &HeaderMap, development: bool) -> Result<String> {
    let name = if development {
        "nodectl_admin_dev"
    } else {
        "__Host-nodectl_admin"
    };
    let mut found = text(h, "cookie")?
        .unwrap_or("")
        .split(';')
        .filter_map(|v| v.trim().split_once('='))
        .filter(|(n, _)| *n == name);
    let value = found.next().context("unauthorized")?.1;
    ensure!(
        found.next().is_none() && value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()),
        "unauthorized"
    );
    Ok(value.into())
}
async fn handle(
    State(app): State<Arc<App>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let audit = app.clone();
    let result = match app.requests.clone().try_acquire_owned() {
        Ok(permit) => {
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                dispatch(&app, &method, &uri, &headers, &body)
            })
            .await
        }
        Err(_) => Ok(Err(anyhow::anyhow!("rate_limited"))),
    };
    let mut response = match result {
        Ok(Ok((value, cookie))) => {
            let mut r = Json(value).into_response();
            if let Some(c) = cookie
                && let Ok(c) = HeaderValue::from_str(&c)
            {
                r.headers_mut().insert("set-cookie", c);
            }
            r
        }
        other => {
            let error = match other {
                Ok(Err(e)) => e.to_string(),
                _ => "internal_error".into(),
            };
            let code = match error.as_str() {
                "unauthorized"
                | "rate_limited"
                | "csrf_denied"
                | "origin_denied"
                | "protected_field"
                | "reauth_denied"
                | "immutable_platform_boundary"
                | "dmit_invariant"
                | "reserved_port"
                | "stale_plan"
                | "advanced_unlock_required"
                | "confirmation_required"
                | "operation_busy"
                | "check_required"
                | "not_deployed"
                | "not_found"
                | "method_not_allowed"
                | "bad_request" => error.as_str(),
                _ => "operation_failed",
            };
            let _ = audit.audit("request-denied", code, None);
            let status = match code {
                "unauthorized" => 401,
                "rate_limited" => 429,
                "csrf_denied"
                | "origin_denied"
                | "reauth_denied"
                | "protected_field"
                | "immutable_platform_boundary"
                | "advanced_unlock_required" => 403,
                "not_found" | "not_deployed" => 404,
                "method_not_allowed" => 405,
                "stale_plan" | "operation_busy" | "check_required" => 409,
                _ => 400,
            };
            (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"error":code,"message":remedy(code)})),
            )
                .into_response()
        }
    };
    for (k, v) in [
        ("cache-control", "private, no-store"),
        ("pragma", "no-cache"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
    ] {
        response
            .headers_mut()
            .insert(k, HeaderValue::from_static(v));
    }
    response
}
fn remedy(code: &str) -> &str {
    match code {
        "unauthorized" => "会话失效或 Token 不正确，请重新登录。",
        "reauth_denied" => "管理员 Token 重新认证失败，请检查后重试；当前会话仍有效。",
        "rate_limited" => "登录尝试过于频繁，请等待一分钟。",
        "origin_denied" | "csrf_denied" => "请求来源或会话校验失败，请刷新页面。",
        "protected_field" => "安全模式禁止修改平台保护配置。请先重新认证进入高级模式。",
        "immutable_platform_boundary" | "dmit_invariant" | "reserved_port" => {
            "变更违反 DMIT 安全约束，高级模式也不能绕过。"
        }
        "stale_plan" => "配置、发布版本或审批已变化，请重新预览。",
        "advanced_unlock_required" => "高级解锁已失效，请重新认证并预览。",
        "confirmation_required" => "请输入所显示的计划或操作标识完成二次确认。",
        "check_required" => "候选配置尚未通过真实运行环境检查。",
        "operation_busy" => "已有管理操作执行中，请查看任务进度。",
        "not_deployed" => "尚无成功部署版本。先通过 nodectl deploy 完成首次部署。",
        "operation_failed" => {
            "检查或操作失败。原始工具输出可能含凭据，已隐藏；请检查配置依赖、证书、DNS 和恢复状态后重试。"
        }
        _ => "请求不可用，请刷新后重试。",
    }
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("bad_request"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    config: Config,
    unlock: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    plan: String,
    confirm: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    operation: String,
    confirm: String,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    zone: String,
    token: String,
}
type Reply = Result<(Value, Option<String>)>;
fn executable() -> Result<PathBuf> {
    // Keep child operations tied to this running API version after an atomic
    // CLI upgrade. A controlled API restart selects the newly installed version.
    #[cfg(target_os = "linux")]
    {
        Ok(PathBuf::from("/proc/self/exe"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(std::env::current_exe()?)
    }
}
fn dispatch(app: &Arc<App>, method: &Method, uri: &Uri, h: &HeaderMap, body: &[u8]) -> Reply {
    ensure!(uri.query().is_none(), "bad_request");
    ensure!(
        method == Method::GET || method == Method::POST,
        "method_not_allowed"
    );
    let path = uri
        .path()
        .strip_prefix("/admin/api/")
        .context("not_found")?;
    let post = method == Method::POST;
    if post {
        ensure!(
            text(h, "origin")? == Some(app.origin.as_str()),
            "origin_denied"
        );
        ensure!(
            text(h, "content-type")?
                .is_some_and(|v| v.split(';').next() == Some("application/json")),
            "bad_request"
        );
    } else {
        ensure!(body.is_empty(), "bad_request");
    }
    if path == "login" && post {
        let login: Login = decode(body)?;
        let result = app.auth.lock().unwrap().login(&app.store, &login.token);
        app.audit(
            "login",
            if result.is_ok() { "success" } else { "denied" },
            None,
        )?;
        let (id, csrf) = result?;
        let name = if app.development {
            "nodectl_admin_dev"
        } else {
            "__Host-nodectl_admin"
        };
        return Ok((
            json!({"csrf":csrf,"expires_in":28800,"idle_timeout":900}),
            Some(format!(
                "{name}={id}; Path=/; HttpOnly; SameSite=Strict; Max-Age=28800{}",
                if app.development { "" } else { "; Secure" }
            )),
        ));
    }
    let id = cookie(h, app.development)?;
    let owner = store::digest(id.as_bytes());
    let csrf = text(h, "x-csrf-token")?;
    {
        let mut auth = app.auth.lock().unwrap();
        auth.session(
            &app.store,
            &id,
            if post {
                Some(csrf.context("csrf_denied")?)
            } else {
                None
            },
        )?;
    }
    if !post {
        let value = match path {
            "session" => {
                let mut a = app.auth.lock().unwrap();
                json!({"csrf":a.session(&app.store,&id,None)?.csrf,"safe_mode":true})
            }
            "overview" => read::overview(&app.store, &mut app.meter.lock().unwrap()),
            "config" => {
                let (c, _) = app.store.inputs()?;
                json!({"config":c,"assets":policy::ASSETS,"base":policy::fingerprint(&app.store)?,"safe_mode":true})
            }
            "nodes" => read::nodes(&app.store)?,
            "clients/xray" | "clients/mihomo" => {
                let _guard = app.store.lock()?;
                let (kind, bytes) = crate::share::current_clients(
                    &app.store,
                    if path.ends_with("xray") {
                        "xray"
                    } else {
                        "mihomo"
                    },
                )?;
                app.audit("export-clients", "success", None)?;
                json!({"content_type":kind,"content":String::from_utf8(bytes)?})
            }
            "network" => read::network(&app.store)?,
            "dns" => read::dns(&app.store)?,
            "services" => {
                json!({"services":read::services(app.store.inputs().ok().as_ref().map(|v|&v.0))})
            }
            "operations" => {
                json!({"jobs":app.job_history(),"audit":app.audit_read()?,"current":app.store.pointer("current")?,"previous":app.store.pointer("previous")?,"recovery_required":(["transaction.json","deployment.json","uninstall.json","admin-input.json"].iter().any(|n|app.store.state_dir.join(n).exists())),"backups":app.backups()?})
            }
            p if p.starts_with("jobs/") => {
                let mut jobs = app.jobs.lock().unwrap();
                let job = jobs.get_mut(&p[5..]).context("not_found")?;
                if job.finished.is_none() {
                    let journal = app.store.state_dir.join("deployment.json");
                    if store::check_private(&journal, false).is_ok()
                        && let Ok(bytes) = fs::read(journal)
                        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                        && let Some(phase) = value["phase"].as_str()
                        && [
                            "snapshot-pending",
                            "backed-up",
                            "warp-probe",
                            "checking",
                            "applying",
                            "applied",
                            "committed",
                            "materials-restored",
                            "restored",
                        ]
                        .contains(&phase)
                    {
                        job.phase = phase.into();
                    }
                }
                let value = serde_json::to_value(&*job)?;
                if let Some(result) = job.result.as_object_mut() {
                    result.remove("token");
                }
                value
            }
            _ => bail!("not_found"),
        };
        return Ok((value, None));
    }
    let value = match path {
        "logout" => {
            decode::<BTreeMap<String, Value>>(body)?;
            app.auth.lock().unwrap().sessions.remove(&owner);
            app.plans.lock().unwrap().retain(|_, p| p.owner != owner);
            json!({"ok":true})
        }
        "unlock" => {
            let input: Login = decode(body)?;
            let mut a = app.auth.lock().unwrap();
            a.attempt()?;
            ensure!(auth::verify(&app.store, &input.token), "reauth_denied");
            let session = a.session(&app.store, &id, csrf)?;
            let ticket = auth::random();
            session.unlock = Some((ticket.clone(), auth::now() + 300));
            app.audit("advanced-unlock", "success", None)?;
            json!({"unlock":ticket,"expires_in":300,"risk":"平台参数变更可能中断 TLS、REALITY、订阅或出口。必须审阅原值、差异并二次确认；不可绕过 DMIT 不变量。"})
        }
        "config/preview" => {
            ensure!(!app.busy.load(Ordering::SeqCst), "operation_busy");
            let input: Preview = decode(body)?;
            let advanced = if let Some(ticket) = &input.unlock {
                let mut a = app.auth.lock().unwrap();
                let s = a.session(&app.store, &id, csrf)?;
                let grant = s.unlock.take().context("advanced_unlock_required")?;
                ensure!(
                    grant.0 == *ticket && grant.1 >= auth::now(),
                    "advanced_unlock_required"
                );
                true
            } else {
                false
            };
            app.preview(input.config, owner, advanced)?
        }
        "config/check" | "config/apply" => {
            let input: Approval = decode(body)?;
            let plan = app.plan(&input.plan, &owner)?;
            let apply = path.ends_with("apply");
            if apply {
                ensure!(
                    input.confirm.as_deref() == Some(plan.id.as_str()),
                    "confirmation_required"
                );
                ensure!(plan.checked, "check_required");
            }
            let operation = if apply {
                "config-apply"
            } else {
                "config-check"
            };
            let job = app.start(operation, move |app, job| {
                app.execute_plan(plan, apply, job)
            })?;
            json!({"job":job})
        }
        "backups/preview" => {
            ensure!(!app.busy.load(Ordering::SeqCst), "operation_busy");
            let input: Restore = decode(body)?;
            {
                let mut a = app.auth.lock().unwrap();
                a.attempt()?;
                ensure!(auth::verify(&app.store, &input.token), "reauth_denied");
            }
            let path = app.snapshot_path(&input.backup)?;
            app.verify_snapshot(&path)?;
            let (c, secrets) = config::load(&path)?;
            let result = app.preview_source(c, owner, true, Some(secrets))?;
            app.audit("backup-restore-preview", "advanced", Some(&input.backup))?;
            result
        }
        "config/deploy-preview" => {
            let input: ProvisionApproval = decode(body)?;
            let plan = app.plan(&input.plan, &owner)?;
            let job = app.start("deployment-preview", move |app, job| {
                app.deployment_preview(plan, input.options, job)
            })?;
            json!({"job":job})
        }
        "credentials" => {
            ensure!(!app.busy.load(Ordering::SeqCst), "operation_busy");
            let input: Credentials = decode(body)?;
            let _guard = app.store.lock()?;
            let cf = crate::dns::Cloudflare::new(&input.zone, input.token.clone())?;
            cf.zone_name()?;
            let dir = app.store.config_dir.join("credentials");
            store::private_dir(&dir)?;
            store::atomic_write(&dir.join("cloudflare-token"), input.token.as_bytes())?;
            store::atomic_write(&dir.join("cloudflare-zone"), input.zone.as_bytes())?;
            app.audit("cloudflare-credentials", "success", None)?;
            json!({"saved":true})
        }
        "operations" => {
            let input: Operation = decode(body)?;
            ensure!(input.confirm == input.operation, "confirmation_required");
            app.validate_operation(&input)?;
            let op = input.operation.clone();
            let id = app.start(&op, move |app, job| app.operation(input, job))?;
            json!({"job":id})
        }
        _ => bail!("not_found"),
    };
    Ok((value, None))
}
impl App {
    fn audit(&self, op: &str, outcome: &str, id: Option<&str>) -> Result<()> {
        let _guard = self.audit_lock.lock().unwrap();
        let dir = auth::directory(&self.store);
        store::private_dir(&dir)?;
        let p = dir.join("audit.jsonl");
        if p.exists() {
            store::check_private(&p, false)?;
            if fs::metadata(&p)?.len() > 4 * 1024 * 1024 {
                fs::rename(&p, dir.join("audit.previous.jsonl"))?;
            }
        }
        let mut options = fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = options.open(p)?;
        writeln!(
            f,
            "{}",
            json!({"time":auth::now(),"operation":op,"outcome":outcome,"id":id})
        )?;
        f.sync_all()?;
        Ok(())
    }
    fn audit_read(&self) -> Result<Vec<Value>> {
        let _g = self.audit_lock.lock().unwrap();
        let p = auth::directory(&self.store).join("audit.jsonl");
        if !p.exists() {
            return Ok(vec![]);
        }
        store::check_private(&p, false)?;
        Ok(fs::read_to_string(p)?
            .lines()
            .rev()
            .take(100)
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect())
    }
    fn snapshot_path(&self, id: &str) -> Result<PathBuf> {
        ensure!(
            id.len() == 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "bad_request"
        );
        Ok(auth::directory(&self.store).join("backups").join(id))
    }
    fn save_snapshot(&self, id: &str) -> Result<PathBuf> {
        let path = self.snapshot_path(id)?;
        self.store.inputs()?;
        store::private_dir(&path)?;
        let mut files = BTreeMap::new();
        for name in ["node.toml", "secrets.json"] {
            let bytes = fs::read(self.store.config_dir.join(name))?;
            files.insert(name.into(), store::digest(&bytes));
            store::atomic_write(&path.join(name), &bytes)?;
        }
        store::atomic_write(
            &path.join("snapshot.json"),
            &serde_json::to_vec(&Snapshot {
                schema_version: 1,
                files,
                current: self.store.pointer("current")?,
            })?,
        )?;
        Ok(path)
    }
    fn verify_snapshot(&self, path: &std::path::Path) -> Result<Snapshot> {
        store::check_private(path, true)?;
        store::check_private(&path.join("snapshot.json"), false)?;
        let snapshot: Snapshot = serde_json::from_slice(&fs::read(path.join("snapshot.json"))?)?;
        ensure!(
            snapshot.schema_version == 1 && snapshot.files.len() == 2,
            "Invalid snapshot"
        );
        for name in ["node.toml", "secrets.json"] {
            store::check_private(&path.join(name), false)?;
            ensure!(
                snapshot.files.get(name) == Some(&store::file_digest(&path.join(name))?),
                "Snapshot integrity mismatch"
            );
        }
        Ok(snapshot)
    }
    fn backups(&self) -> Result<Vec<Value>> {
        let dir = auth::directory(&self.store).join("backups");
        if !dir.exists() {
            return Ok(vec![]);
        }
        store::check_private(&dir, true)?;
        let mut values = vec![];
        for entry in fs::read_dir(dir)? {
            let e = entry?;
            let id = e.file_name().to_string_lossy().into_owned();
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let restorable = self.verify_snapshot(&e.path()).is_ok();
            values.push(json!({"id":id,"created":e.metadata()?.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs(),"restorable":restorable}));
        }
        values.sort_by_key(|v| std::cmp::Reverse(v["created"].as_u64().unwrap_or(0)));
        values.truncate(100);
        Ok(values)
    }
    fn preview(&self, candidate: Config, owner: String, advanced: bool) -> Result<Value> {
        self.preview_source(candidate, owner, advanced, None)
    }
    fn preview_source(
        &self,
        candidate: Config,
        owner: String,
        advanced: bool,
        restore_secrets: Option<config::Secrets>,
    ) -> Result<Value> {
        let _guard = self.store.lock()?;
        let (old, existing) = self.store.inputs()?;
        let secrets = restore_secrets.as_ref().unwrap_or(&existing);
        config::validate(&candidate, secrets)?;
        let mut changes = policy::enforce(&old, &candidate, advanced)?;
        let restores_identity = restore_secrets.is_some();
        if restores_identity {
            ensure!(advanced, "advanced_unlock_required");
            changes.push(json!({"field":"secrets","before":"<redacted>","after":"<restored from protected snapshot>","protected":true}));
        }
        {
            let mut plans = self.plans.lock().unwrap();
            plans.retain(|_, p| p.expires >= auth::now());
            ensure!(plans.len() < 16, "operation_busy");
        }
        if !self.development {
            let origin: Uri = self.origin.parse()?;
            if let Some(port) = origin.port_u16() {
                ensure!(
                    ![
                        candidate.ports.http,
                        candidate.ports.https,
                        candidate.ports.reality
                    ]
                    .contains(&port)
                        && !candidate.web_services.as_ref().is_some_and(|w| [
                            w.librespeed_port,
                            w.openlist_port
                        ]
                        .contains(&port)),
                    "reserved_port"
                );
            }
            ensure!(
                candidate
                    .domains()
                    .iter()
                    .any(|d| Some(d.as_str()) == origin.host()),
                "Retain the administrator HTTPS hostname as a node alias before changing domains"
            );
        }
        let id = auth::random();
        let path = auth::directory(&self.store).join("plans").join(&id);
        store::private_dir(&path)?;
        store::atomic_write(
            &path.join("node.toml"),
            toml::to_string_pretty(&candidate)?.as_bytes(),
        )?;
        store::atomic_write(&path.join("secrets.json"), &serde_json::to_vec(&secrets)?)?;
        let stage = Store::new(path, self.store.state_dir.clone())?;
        let release = stage.render()?;
        let diff = stage.diff(&release)?;
        let plan = Plan {
            id: id.clone(),
            release: release.clone(),
            owner,
            base: policy::fingerprint(&self.store)?,
            expires: auth::now() + 600,
            candidate,
            advanced,
            checked: false,
            changes: changes.clone(),
            diff: diff.clone(),
            generation: auth::generation(&self.store)?,
            deployment: None,
            restore_secrets,
        };
        let mut plans = self.plans.lock().unwrap();
        plans.retain(|_, p| p.expires >= auth::now());
        ensure!(plans.len() < 16, "operation_busy");
        plans.insert(id.clone(), plan);
        self.audit(
            "config-preview",
            if advanced { "advanced" } else { "safe" },
            Some(&id),
        )?;
        Ok(
            json!({"plan":id,"release":release,"changes":changes,"diff":diff,"expires_in":600,"advanced":advanced,"restores_identity":restores_identity,"risk":if restores_identity{"恢复包含客户端身份，应用后可能需要更新客户端。管理员与订阅 Token 不参与恢复；DNS/证书不匹配时必须先处理依赖。"}else{"域名/IP 变更要求 DNS 和双证书 SAN 先满足依赖；检查不通过不会应用。"}}),
        )
    }
    fn plan(&self, id: &str, owner: &str) -> Result<Plan> {
        let plan = self
            .plans
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("stale_plan")?;
        ensure!(
            plan.generation == auth::generation(&self.store)?
                && plan.owner == owner
                && plan.expires >= auth::now()
                && plan.base == policy::fingerprint(&self.store)?,
            "stale_plan"
        );
        Ok(plan)
    }
    fn start<F>(self: &Arc<Self>, operation: &str, work: F) -> Result<String>
    where
        F: FnOnce(&Arc<Self>, &str) -> Result<Value> + Send + 'static,
    {
        ensure!(
            self.busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "operation_busy"
        );
        let id = auth::random();
        if let Err(e) = self.audit(operation, "started", Some(&id)) {
            self.busy.store(false, Ordering::SeqCst);
            return Err(e);
        }
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.len() >= 100 {
                let first = jobs
                    .values()
                    .min_by_key(|j| j.started)
                    .map(|j| j.id.clone());
                if let Some(first) = first {
                    jobs.remove(&first);
                }
            }
            jobs.insert(
                id.clone(),
                Job {
                    id: id.clone(),
                    operation: operation.into(),
                    phase: "queued".into(),
                    started: auth::now(),
                    finished: None,
                    ok: None,
                    result: Value::Null,
                },
            );
        }
        if let Err(error) = self.save_jobs() {
            self.busy.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let app = self.clone();
        let job = id.clone();
        let op = operation.to_owned();
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&app, &job)));
            let (ok, value) = match result {
                Ok(Ok(v)) => (true, v),
                _ => (
                    false,
                    json!({"error":"operation_failed","message":remedy("operation_failed")}),
                ),
            };
            let _ = app.audit(&op, if ok { "success" } else { "failed" }, Some(&job));
            if let Some(j) = app.jobs.lock().unwrap().get_mut(&job) {
                j.finished = Some(auth::now());
                j.ok = Some(ok);
                j.phase = if ok { "completed" } else { "failed" }.into();
                j.result = value;
            }
            let _ = app.save_jobs();
            // Once restart is acknowledged, admit no new writes in the timer
            // window. The new API process releases this admission barrier.
            if !(ok && op == "api-restart") {
                app.busy.store(false, Ordering::SeqCst);
            }
        });
        Ok(id)
    }
    fn phase(&self, id: &str, phase: &str) {
        if let Some(j) = self.jobs.lock().unwrap().get_mut(id) {
            j.phase = phase.into();
        }
        let _ = self.save_jobs();
    }
    fn reference(&self) -> Result<host::Reference> {
        let id = self.store.pointer("current")?.context("not_deployed")?;
        let p = self.store.state_dir.join("checks").join(id);
        store::check_private(&p, false)?;
        let value: Value = serde_json::from_slice(&fs::read(p)?)?;
        Ok(serde_json::from_value(value["reference"].clone())?)
    }
    fn execute_plan(&self, plan: Plan, apply: bool, job: &str) -> Result<Value> {
        if apply && let Some(deployment) = &plan.deployment {
            self.plan(&plan.id, &plan.owner)?;
            ensure!(plan.checked, "check_required");
            self.phase(job, "deployment-backup-dns-certificates-check-apply");
            let args =
                self.deployment_args(&plan, &deployment.options, Some(&deployment.digest))?;
            let (_, success) = self.child(&args)?;
            ensure!(success, "deployment_failed");
            self.plans.lock().unwrap().remove(&plan.id);
            return Ok(json!({"current":self.store.pointer("current")?,"deployment":true}));
        }
        let _guard = self.store.lock()?;
        let _lease = crate::provision::Lease::hold(&_guard);
        self.plan(&plan.id, &plan.owner)?;
        let (old, existing) = self.store.inputs()?;
        let secrets = plan.restore_secrets.as_ref().unwrap_or(&existing);
        config::validate(&plan.candidate, secrets)?;
        policy::enforce(&old, &plan.candidate, plan.advanced)?;
        if !apply {
            {
                let mut plans = self.plans.lock().unwrap();
                let target = plans.get_mut(&plan.id).context("stale_plan")?;
                target.checked = false;
                target.deployment = None;
            }
            self.phase(job, "check");
            crate::dns::verify(&plan.candidate)?;
            host::check(&self.store, &plan.release, self.reference()?)?;
            self.plans
                .lock()
                .unwrap()
                .get_mut(&plan.id)
                .context("stale_plan")?
                .checked = true;
            self.audit("config-check", "passed", Some(&plan.id))?;
            return Ok(json!({"plan":plan.id,"checked":true}));
        }
        ensure!(plan.checked, "check_required");
        crate::dns::verify(&plan.candidate)?;
        self.phase(job, "snapshot");
        let journal = policy::InputJournal {
            before: fs::read_to_string(self.store.config_dir.join("node.toml"))?,
            candidate: plan.release.clone(),
            previous: self.store.pointer("current")?,
            before_secrets: if plan.restore_secrets.is_some() {
                Some(fs::read_to_string(
                    self.store.config_dir.join("secrets.json"),
                )?)
            } else {
                None
            },
        };
        let dir = self.save_snapshot(&plan.id)?;
        store::atomic_write(
            &dir.join("change.json"),
            &serde_json::to_vec(
                &json!({"changes":plan.changes,"diff":plan.diff,"advanced":plan.advanced,"release":plan.release}),
            )?,
        )?;
        store::atomic_write(
            &self.store.state_dir.join("admin-input.json"),
            &serde_json::to_vec(&journal)?,
        )?;
        self.phase(job, "apply");
        let result = host::apply(&self.store, &plan.release);
        self.phase(job, "recover-inputs");
        policy::recover_inputs(&self.store)?;
        result?;
        self.plans.lock().unwrap().remove(&plan.id);
        Ok(json!({"release":plan.release,"backup":plan.id,"restorable":true}))
    }
    fn validate_operation(&self, input: &Operation) -> Result<()> {
        match input.operation.as_str() {
            "doctor"
            | "dns-check"
            | "network-check"
            | "renew-certificates"
            | "backup"
            | "rollback"
            | "recover"
            | "subscription-rotate"
            | "subscription-revoke"
            | "logs" => {}
            "api-restart" => {}
            "service-restart" | "service-reload" => {
                let service = input.service.as_deref().context("bad_request")?;
                ensure!(read::SERVICES.contains(&service), "bad_request");
                if input.operation == "service-reload" {
                    ensure!(service == "nginx", "bad_request");
                }
            }
            "upgrade" => {
                let v = input.version.as_deref().context("bad_request")?;
                ensure!(
                    regex::Regex::new(r"^v?[0-9]+\.[0-9]+\.[0-9]+$")?.is_match(v),
                    "bad_request"
                );
                let version = v
                    .trim_start_matches('v')
                    .split('.')
                    .map(str::parse::<u64>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ensure!(
                    version.as_slice() >= [0, 6, 0].as_slice(),
                    "Web upgrades require nodectl v0.6.0 or newer; use the CLI for deliberate legacy tool restoration"
                );
            }
            _ => bail!("bad_request"),
        }
        Ok(())
    }
    fn operation(&self, input: Operation, job: &str) -> Result<Value> {
        self.phase(job, &input.operation);
        if input.operation == "upgrade" {
            crate::upgrade::run(
                &self.store,
                input.version.as_deref().unwrap(),
                std::path::Path::new("/usr/local"),
                true,
            )?;
            return Ok(
                json!({"installed":input.version,"message":"新 CLI 已验签安装；当前 API 进程继续使用原版本，需由 systemd 重新启动。"}),
            );
        }
        let _guard = if input.operation == "recover" {
            self.store.recovery_lock()?
        } else {
            self.store.lock()?
        };
        let _lease = crate::provision::Lease::hold(&_guard);
        match input.operation.as_str() {
            "api-restart" => {
                host::command(
                    "systemd-run",
                    &[
                        "--unit=nodectl-admin-restart",
                        "--on-active=3s",
                        "--timer-property=AccuracySec=100ms",
                        "--timer-property=RandomizedDelaySec=0",
                        "--collect",
                        "/usr/bin/systemctl",
                        "restart",
                        "nodectl-admin",
                    ],
                )?;
                Ok(
                    json!({"message":"管理 API 将在三秒后重启，当前会话失效。请使用现有管理员 Token 重新登录。"}),
                )
            }
            "doctor" => {
                let output = self.cli(&["doctor", "--json"])?;
                Ok(serde_json::from_str(&output)?)
            }
            "dns-check" => {
                let (c, _) = self.store.inputs()?;
                crate::dns::verify(&c)?;
                Ok(json!({"verified":true}))
            }
            "network-check" => {
                let (c, _) = self.store.inputs()?;
                let mut result = vec![];
                for (name, interface) in [
                    ("native", None),
                    ("warp", c.warp.enabled.then_some("tun0")),
                    (
                        "cn",
                        c.cn_warp
                            .as_ref()
                            .filter(|cn| cn.enabled)
                            .map(|cn| cn.interface.as_str()),
                    ),
                ] {
                    if name != "native" && interface.is_none() {
                        continue;
                    }
                    let mut args = vec![
                        "--disable",
                        "--fail",
                        "--silent",
                        "--show-error",
                        "--max-time",
                        "15",
                        "--proto",
                        "=https",
                    ];
                    if let Some(i) = interface {
                        args.extend(["--interface", i]);
                    }
                    args.push("https://www.cloudflare.com/cdn-cgi/trace");
                    let trace = host::command("curl", &args);
                    result.push(json!({"name":name,"ok":trace.is_ok(),"trace":trace.ok().map(|s|s.lines().filter(|l|l.starts_with("ip=")||l.starts_with("loc=")||l.starts_with("warp=")).collect::<Vec<_>>().join("\n"))}));
                }
                Ok(json!({"diagnostics":result}))
            }
            "renew-certificates" => {
                host::renew(&self.store)?;
                Ok(
                    json!({"verified_and_reloaded":true,"message":"已执行现有 renew-hook；签发/续期由既有 ACME 定时任务管理。"}),
                )
            }
            "backup" => {
                let id = auth::random();
                self.save_snapshot(&id)?;
                Ok(json!({"backup":id}))
            }
            "subscription-rotate" => {
                crate::share::rotate(&self.store, None)?;
                let p = crate::share::credential_path(&self.store);
                let v: Value = serde_json::from_slice(&fs::read(p)?)?;
                Ok(
                    json!({"token":v["token"],"message":"旧订阅 Token 立即失效。请保存新 Token；管理员会话不受影响。"}),
                )
            }
            "subscription-revoke" => {
                crate::share::revoke(&self.store)?;
                Ok(json!({"revoked":true}))
            }
            "service-restart" | "service-reload" => {
                let service = input.service.as_deref().unwrap();
                let (c, _) = self.store.inputs()?;
                ensure!(
                    match service {
                        "usque-cn" => c.routes_cn(),
                        "usque-warp" => c.warp.enabled,
                        "librespeed" | "openlist" => c.serves_web(),
                        "nodectl-share" => c.distributes(),
                        _ => true,
                    },
                    "bad_request"
                );
                if service == "nginx" {
                    host::command(
                        &c.runtime.nginx_binary,
                        &["-t", "-c", "/etc/nginx/nginx.conf"],
                    )?;
                }
                host::command(
                    "systemctl",
                    &[
                        if input.operation == "service-reload" {
                            "reload"
                        } else {
                            "restart"
                        },
                        service,
                    ],
                )?;
                host::command("systemctl", &["is-active", "--quiet", service])?;
                Ok(json!({"services":read::services(Some(&c))}))
            }
            "rollback" => {
                let previous = self.store.pointer("previous")?.context("not_deployed")?;
                if !crate::deploy::rollback(&self.store, self.cf_token().as_deref())? {
                    let journal = policy::InputJournal {
                        before: fs::read_to_string(self.store.config_dir.join("node.toml"))?,
                        candidate: previous.clone(),
                        previous: self.store.pointer("current")?,
                        before_secrets: Some(fs::read_to_string(
                            self.store.config_dir.join("secrets.json"),
                        )?),
                    };
                    store::atomic_write(
                        &self.store.state_dir.join("admin-input.json"),
                        &serde_json::to_vec(&journal)?,
                    )?;
                    let result = host::apply(&self.store, &previous);
                    policy::recover_inputs(&self.store)?;
                    result?;
                }
                Ok(json!({"current":self.store.pointer("current")?}))
            }
            "recover" => {
                if self.store.state_dir.join("deployment.json").exists() {
                    crate::deploy::recover(&self.store, self.cf_token().as_deref())?;
                } else if self.store.state_dir.join("transaction.json").exists()
                    || self.store.state_dir.join("uninstall.json").exists()
                {
                    host::recover(&self.store)?;
                }
                policy::recover_inputs(&self.store)?;
                Ok(json!({"recovered":true}))
            }
            "logs" => {
                let output = host::command(
                    "journalctl",
                    &[
                        "-u",
                        "nginx",
                        "-u",
                        "xray",
                        "-u",
                        "usque-warp",
                        "-u",
                        "usque-cn",
                        "-u",
                        "librespeed",
                        "-u",
                        "openlist",
                        "-n",
                        "100",
                        "--no-pager",
                        "--output=short-iso",
                    ],
                )?;
                let redacted = redact_log(&self.store, output)?;
                Ok(json!({"log":redacted}))
            }
            _ => bail!("bad_request"),
        }
    }
    fn job_history(&self) -> Vec<Job> {
        let mut jobs: Vec<Job> = self
            .jobs
            .lock()
            .unwrap()
            .values()
            .cloned()
            .map(|mut j| {
                j.result = Value::Null;
                j
            })
            .collect();
        jobs.sort_by_key(|j| std::cmp::Reverse(j.started));
        jobs
    }
    fn save_jobs(&self) -> Result<()> {
        store::atomic_write(
            &auth::directory(&self.store).join("jobs.json"),
            &serde_json::to_vec(&self.job_history())?,
        )
    }
    fn deployment_args(
        &self,
        plan: &Plan,
        options: &ProvisionOptions,
        approve: Option<&str>,
    ) -> Result<Vec<String>> {
        ensure!(
            plan.restore_secrets.is_none(),
            "Snapshot identity restoration requires the direct checked apply flow"
        );
        let input = auth::directory(&self.store)
            .join("plans")
            .join(&plan.id)
            .join("node.toml");
        store::check_private(&input, false)?;
        let staged: Config = toml::from_str(&fs::read_to_string(&input)?)?;
        ensure!(
            serde_json::to_value(staged)? == serde_json::to_value(&plan.candidate)?,
            "stale_plan"
        );
        let mut args = vec![
            "deploy".into(),
            "--non-interactive".into(),
            "--input".into(),
            input.to_str().context("path")?.into(),
        ];
        if options.manual_certificates {
            args.push("--manual-certificates".into());
        }
        if options.skip_dependencies {
            args.push("--skip-dependencies".into());
        }
        if options.accept_ca_terms {
            args.push("--accept-ca-terms".into());
        }
        if options.accept_warp_terms {
            args.push("--accept-warp-terms".into());
        }
        if let Some(email) = &options.acme_email {
            ensure!(
                email.len() < 255 && !email.starts_with('-') && !email.contains(['\n', '\r']),
                "bad_request"
            );
            args.extend(["--acme-email".into(), email.clone()]);
        }
        let dir = self.store.config_dir.join("credentials");
        if dir.join("cloudflare-zone").exists() {
            store::check_private(&dir.join("cloudflare-zone"), false)?;
            let zone = fs::read_to_string(dir.join("cloudflare-zone"))?;
            ensure!(
                zone.len() == 32 && zone.bytes().all(|b| b.is_ascii_hexdigit()),
                "bad_request"
            );
            let token = dir.join("cloudflare-token");
            store::check_private(&token, false)?;
            args.extend([
                "--dns-zone".into(),
                zone,
                "--token-file".into(),
                token.to_str().context("path")?.into(),
            ]);
        }
        if let Some(digest) = approve {
            args.extend(["--execute".into(), "--approve".into(), digest.into()]);
        }
        Ok(args)
    }
    fn deployment_preview(
        &self,
        plan: Plan,
        options: ProvisionOptions,
        job: &str,
    ) -> Result<Value> {
        self.plan(&plan.id, &plan.owner)?;
        {
            let mut plans = self.plans.lock().unwrap();
            let current = plans.get_mut(&plan.id).context("stale_plan")?;
            current.checked = false;
            current.deployment = None;
        }
        self.phase(job, "deployment-inventory-dns-plan");
        let args = self.deployment_args(&plan, &options, None)?;
        let (output, success) = self.child(&args)?;
        ensure!(success, "deployment_preview_failed");
        let start = output.find('{').context("deployment_plan_missing")?;
        let value: Value = serde_json::Deserializer::from_str(&output[start..])
            .into_iter::<Value>()
            .next()
            .context("deployment_plan_missing")??;
        ensure!(
            value["required_actions"]
                .as_array()
                .is_some_and(|v| v.is_empty()),
            "deployment_prerequisites"
        );
        ensure!(
            value["configuration"] == serde_json::to_value(&plan.candidate)?,
            "stale_plan"
        );
        let digest = store::digest(&serde_json::to_vec(&value)?);
        ensure!(
            output.contains(&format!("Plan SHA256: {digest}")),
            "deployment_plan_digest"
        );
        self.plan(&plan.id, &plan.owner)?;
        let mut plans = self.plans.lock().unwrap();
        let target = plans.get_mut(&plan.id).context("stale_plan")?;
        target.deployment = Some(Deployment {
            options,
            digest: digest.clone(),
        });
        target.checked = true;
        Ok(
            json!({"plan":plan.id,"deployment_digest":digest,"deployment":value,"check_deferred_until_dependencies":true}),
        )
    }
    fn child(&self, args: &[String]) -> Result<(String, bool)> {
        use std::io::{Read, Seek, SeekFrom};
        let mut stdout = tempfile::tempfile()?;
        let stderr = tempfile::tempfile()?;
        let mut command = std::process::Command::new(executable()?);
        command
            .arg("--config-dir")
            .arg(&self.store.config_dir)
            .arg("--state-dir")
            .arg(&self.store.state_dir)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr);
        crate::provision::inherit_lease(&mut command);
        #[cfg(feature = "integration-test")]
        if std::env::var("NODECTL_TEST_DNS").as_deref() == Ok("yes") {
            command.env("NODECTL_TEST_DNS", "yes");
        }
        let status = command.status()?;
        stdout.seek(SeekFrom::Start(0))?;
        let mut bytes = vec![];
        stdout.take(2 * 1024 * 1024).read_to_end(&mut bytes)?;
        Ok((String::from_utf8(bytes)?, status.success()))
    }
    fn cf_token(&self) -> Option<PathBuf> {
        let p = self.store.config_dir.join("credentials/cloudflare-token");
        p.exists().then_some(p)
    }
    fn cli(&self, args: &[&str]) -> Result<String> {
        // doctor is read-only and does not acquire Store::lock; its JSON findings
        // deliberately exclude raw external command output.
        let args: Vec<String> = args.iter().map(|v| (*v).into()).collect();
        Ok(self.child(&args)?.0)
    }
}
