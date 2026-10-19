use super::*;
fn sandbox() -> (tempfile::TempDir, Arc<App>, String) {
    let (t, s) = crate::tests::sandbox();
    let token = auth::reset(&s, false).unwrap().unwrap();
    let app = Arc::new(App {
        store: s,
        origin: "https://speedtest.example.com:9443".into(),
        development: false,
        auth: Mutex::new(auth::Auth::default()),
        meter: Mutex::new(read::Meter::default()),
        plans: Mutex::new(BTreeMap::new()),
        jobs: Mutex::new(BTreeMap::new()),
        busy: AtomicBool::new(false),
        audit_lock: Mutex::new(()),
        requests: Arc::new(tokio::sync::Semaphore::new(16)),
    });
    (t, app, token)
}
fn headers(app: &App, id: &str, csrf: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("origin", app.origin.parse().unwrap());
    h.insert("content-type", "application/json".parse().unwrap());
    h.insert(
        "cookie",
        format!("__Host-nodectl_admin={id}").parse().unwrap(),
    );
    h.insert("x-csrf-token", csrf.parse().unwrap());
    h
}
#[test]
fn acknowledged_restart_does_not_admit_new_writes_before_exit() {
    let (_t, app, token) = sandbox();
    let (id, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    let h = headers(&app, &id, &csrf);
    let job = app
        .start("api-restart", |_, _| Ok(json!({"message":"scheduled"})))
        .unwrap();
    for _ in 0..300 {
        if app.jobs.lock().unwrap()[&job].finished.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(app.jobs.lock().unwrap()[&job].ok, Some(true));
    let config = app.store.inputs().unwrap().0;
    assert_eq!(
        call(&app, "config/preview", &h, json!({"config":config}))
            .unwrap_err()
            .to_string(),
        "operation_busy"
    );
    assert!(
        call(
            &app,
            "operations",
            &h,
            json!({"operation":"backup","confirm":"backup"})
        )
        .is_err()
    );
    assert!(call(&app, "logout", &h, json!({})).is_ok());
}
#[test]
fn failed_dependency_review_invalidates_the_previous_check() {
    let (_t, app, token) = sandbox();
    let (id, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    let h = headers(&app, &id, &csrf);
    let c = app.store.inputs().unwrap().0;
    let (view, _) = call(&app, "config/preview", &h, json!({"config":c})).unwrap();
    let plan_id = view["plan"].as_str().unwrap();
    app.plans.lock().unwrap().get_mut(plan_id).unwrap().checked = true;
    let plan = app.plan(plan_id, &store::digest(id.as_bytes())).unwrap();
    let options = ProvisionOptions {
        manual_certificates: true,
        skip_dependencies: true,
        acme_email: Some("-invalid".into()),
        accept_ca_terms: false,
        accept_warp_terms: false,
    };
    assert!(
        app.deployment_preview(plan, options, "test-review")
            .is_err()
    );
    assert_eq!(
        call(
            &app,
            "config/apply",
            &h,
            json!({"plan":plan_id,"confirm":plan_id})
        )
        .unwrap_err()
        .to_string(),
        "check_required"
    );
}
#[test]
fn uninstall_interruption_is_visible_and_never_reported_as_fake_recovery() {
    let (_t, app, token) = sandbox();
    let (id, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    let h = headers(&app, &id, &csrf);
    let path = app.store.state_dir.join("uninstall.json");
    store::atomic_write(&path, b"{}").unwrap();
    let (view, _) = dispatch(
        &app,
        &Method::GET,
        &"/admin/api/operations".parse().unwrap(),
        &h,
        b"",
    )
    .unwrap();
    assert_eq!(view["recovery_required"], true);
    let input = Operation {
        operation: "recover".into(),
        confirm: "recover".into(),
        service: None,
        version: None,
    };
    assert!(app.operation(input, "recovery-test").is_err());
    assert_eq!(fs::read(path).unwrap(), b"{}");
}
fn call(app: &Arc<App>, path: &str, h: &HeaderMap, data: Value) -> Reply {
    dispatch(
        app,
        &Method::POST,
        &format!("/admin/api/{path}").parse().unwrap(),
        h,
        &serde_json::to_vec(&data).unwrap(),
    )
}
#[test]
fn api_requires_origin_session_csrf_and_independent_token() {
    let (_t, app, token) = sandbox();
    let mut h = HeaderMap::new();
    h.insert("content-type", "application/json".parse().unwrap());
    assert!(call(&app, "login", &h, json!({"token":token})).is_err());
    h.insert("origin", app.origin.parse().unwrap());
    crate::share::rotate(&app.store, None).unwrap();
    let subscription: Value =
        serde_json::from_slice(&fs::read(crate::share::credential_path(&app.store)).unwrap())
            .unwrap();
    assert!(call(&app, "login", &h, json!({"token":subscription["token"]})).is_err());
    let (value, cookie) = call(&app, "login", &h, json!({"token":token})).unwrap();
    let cookie = cookie.unwrap();
    assert!(cookie.contains("HttpOnly; SameSite=Strict"));
    assert!(cookie.ends_with("; Secure"));
    let id = cookie.split(';').next().unwrap().split_once('=').unwrap().1;
    let mut h = headers(&app, id, value["csrf"].as_str().unwrap());
    assert!(call(&app, "logout", &h, json!({})).is_ok());
    assert!(call(&app, "unlock", &h, json!({"token":token})).is_err());
    let (id, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    h = headers(&app, &id, &csrf);
    h.insert("origin", "https://attacker.test".parse().unwrap());
    assert!(call(&app, "logout", &h, json!({})).is_err());
    h.insert("origin", app.origin.parse().unwrap());
    h.remove("x-csrf-token");
    assert!(call(&app, "logout", &h, json!({})).is_err());
    let audit = serde_json::to_string(&app.audit_read().unwrap()).unwrap();
    assert!(!audit.contains(&token));
    assert!(!audit.contains(id.as_str()));
}
#[test]
fn backend_rejects_protected_changes_and_binds_review_to_session_and_bytes() {
    let (_t, app, token) = sandbox();
    let (id, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    let h = headers(&app, &id, &csrf);
    let original = fs::read(app.store.config_dir.join("node.toml")).unwrap();
    let secret = fs::read(app.store.config_dir.join("secrets.json")).unwrap();
    let (mut c, _) = app.store.inputs().unwrap();
    c.clients.xhttp_weight = 75;
    c.clients.ws_weight = 20;
    assert_eq!(
        call(&app, "config/preview", &h, json!({"config":c}))
            .unwrap_err()
            .to_string(),
        "protected_field"
    );
    assert_eq!(
        fs::read(app.store.config_dir.join("node.toml")).unwrap(),
        original
    );
    let (ticket, _) = call(&app, "unlock", &h, json!({"token":token})).unwrap();
    let (plan, _) = call(
        &app,
        "config/preview",
        &h,
        json!({"config":c,"unlock":ticket["unlock"]}),
    )
    .unwrap();
    assert!(
        plan["diff"].as_str().unwrap().contains("<redacted>")
            || !plan["diff"].as_str().unwrap().contains("0123456789abcdef")
    );
    let plan_id = plan["plan"].as_str().unwrap();
    assert!(
        call(
            &app,
            "config/preview",
            &h,
            json!({"config":c,"unlock":ticket["unlock"]})
        )
        .is_err()
    );
    assert_eq!(
        call(
            &app,
            "config/apply",
            &h,
            json!({"plan":plan_id,"confirm":plan_id})
        )
        .unwrap_err()
        .to_string(),
        "check_required"
    );
    assert_eq!(
        call(
            &app,
            "config/apply",
            &h,
            json!({"plan":plan_id,"confirm":"yes"})
        )
        .unwrap_err()
        .to_string(),
        "confirmation_required"
    );
    let (other, _) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    assert!(app.plan(plan_id, &store::digest(other.as_bytes())).is_err());
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n# Concurrent CLI edit\n");
    store::atomic_write(&app.store.config_dir.join("node.toml"), &changed).unwrap();
    assert!(app.plan(plan_id, &store::digest(id.as_bytes())).is_err());
    assert_eq!(
        fs::read(app.store.config_dir.join("secrets.json")).unwrap(),
        secret
    );
}
#[test]
fn input_journal_blocks_cli_and_recovers_exact_original_or_committed_release() {
    let (_t, app, _) = sandbox();
    let original = fs::read_to_string(app.store.config_dir.join("node.toml")).unwrap();
    let id = app.store.render().unwrap();
    let p = app.store.state_dir.join("admin-input.json");
    let journal = policy::InputJournal {
        before: format!("{original}\n# preserved operator comment\n"),
        candidate: id.clone(),
        previous: None,
        before_secrets: None,
    };
    store::atomic_write(&p, &serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(app.store.lock().is_err());
    policy::recover_inputs(&app.store).unwrap();
    assert_eq!(
        fs::read_to_string(app.store.config_dir.join("node.toml")).unwrap(),
        journal.before
    );
    assert!(app.store.lock().is_ok());
    store::atomic_write(&p, &serde_json::to_vec(&journal).unwrap()).unwrap();
    app.store.set_pointer("current", Some(&id)).unwrap();
    policy::recover_inputs(&app.store).unwrap();
    assert_eq!(
        fs::read(app.store.config_dir.join("node.toml")).unwrap(),
        fs::read(app.store.release(&id).unwrap().join("source/node.toml")).unwrap()
    );
}
#[test]
fn snapshot_restore_reauthenticates_verifies_integrity_and_never_returns_keys() {
    let (_t, app, token) = sandbox();
    let id = auth::random();
    let path = app.save_snapshot(&id).unwrap();
    let old = fs::read(app.store.config_dir.join("secrets.json")).unwrap();
    let mut changed: Value = serde_json::from_slice(&old).unwrap();
    changed["xhttp_uuid"] = json!(uuid::Uuid::new_v4().to_string());
    store::atomic_write(
        &app.store.config_dir.join("secrets.json"),
        &serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let (session, csrf) = app.auth.lock().unwrap().login(&app.store, &token).unwrap();
    let h = headers(&app, &session, &csrf);
    assert_eq!(
        call(
            &app,
            "backups/preview",
            &h,
            json!({"backup":id,"token":"wrong"})
        )
        .unwrap_err()
        .to_string(),
        "reauth_denied"
    );
    let (view, _) = call(
        &app,
        "backups/preview",
        &h,
        json!({"backup":id,"token":token}),
    )
    .unwrap();
    assert_eq!(view["restores_identity"], true);
    assert!(
        view["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["field"] == "secrets")
    );
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains(changed["xhttp_uuid"].as_str().unwrap())
    );
    let plan = app
        .plan(
            view["plan"].as_str().unwrap(),
            &store::digest(session.as_bytes()),
        )
        .unwrap();
    assert!(plan.restore_secrets.is_some());
    assert!(
        app.deployment_args(
            &plan,
            &ProvisionOptions {
                manual_certificates: true,
                acme_email: None,
                accept_ca_terms: false,
                accept_warp_terms: false,
                skip_dependencies: true
            },
            None
        )
        .is_err()
    );
    store::atomic_write(&path.join("node.toml"), b"tampered").unwrap();
    assert!(app.verify_snapshot(&path).is_err());
    assert!(
        call(
            &app,
            "backups/preview",
            &h,
            json!({"backup":id,"token":token})
        )
        .is_err()
    );
    assert!(app.snapshot_path("../outside").is_err());
    assert_eq!(
        fs::read(app.store.config_dir.join("secrets.json")).unwrap(),
        serde_json::to_vec(&changed).unwrap()
    );
}
#[test]
fn rollback_input_recovery_restores_identity_without_reviving_access_tokens() {
    let (_t, app, _) = sandbox();
    let old = app.store.render().unwrap();
    let old_secret =
        fs::read(app.store.release(&old).unwrap().join("source/secrets.json")).unwrap();
    let mut changed: Value = serde_json::from_slice(&old_secret).unwrap();
    changed["xhttp_uuid"] = json!(uuid::Uuid::new_v4().to_string());
    let changed = serde_json::to_vec_pretty(&changed).unwrap();
    store::atomic_write(&app.store.config_dir.join("secrets.json"), &changed).unwrap();
    let current = app.store.render().unwrap();
    app.store.set_pointer("current", Some(&current)).unwrap();
    crate::share::rotate(&app.store, None).unwrap();
    let access = fs::read(crate::share::credential_path(&app.store)).unwrap();
    let admin = fs::read(auth::directory(&app.store).join("credential.json")).unwrap();
    let journal = policy::InputJournal {
        before: fs::read_to_string(app.store.config_dir.join("node.toml")).unwrap(),
        candidate: old.clone(),
        previous: Some(current),
        before_secrets: Some(String::from_utf8(changed).unwrap()),
    };
    store::atomic_write(
        &app.store.state_dir.join("admin-input.json"),
        &serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    app.store.set_pointer("current", Some(&old)).unwrap();
    policy::recover_inputs(&app.store).unwrap();
    assert_eq!(
        fs::read(app.store.config_dir.join("secrets.json")).unwrap(),
        old_secret
    );
    assert_eq!(
        fs::read(crate::share::credential_path(&app.store)).unwrap(),
        access
    );
    assert_eq!(
        fs::read(auth::directory(&app.store).join("credential.json")).unwrap(),
        admin
    );
}
#[test]
fn service_logs_redact_old_and_current_identity_and_credential_shapes() {
    let (_t, app, _) = sandbox();
    let (_, secrets) = app.store.inputs().unwrap();
    let text = format!(
        "status active {} bearer {} private {}",
        secrets.reality_private_key,
        "a".repeat(64),
        "WARP_PRIVATE_CREDENTIAL_".repeat(3)
    );
    let redacted = redact_log(&app.store, text).unwrap();
    assert!(redacted.contains("status active"));
    assert!(!redacted.contains(&secrets.reality_private_key));
    assert!(!redacted.contains(&"a".repeat(64)));
    assert!(!redacted.contains("WARP_PRIVATE_CREDENTIAL"));
}
#[test]
fn operations_have_no_shell_or_arbitrary_service_url_path_escape() {
    let (_t, app, _) = sandbox();
    for op in ["bash", "exec", "dns-write", "file-read", "reboot"] {
        assert!(
            app.validate_operation(&Operation {
                operation: op.into(),
                confirm: op.into(),
                service: None,
                version: None
            })
            .is_err()
        );
    }
    for s in ["ssh", "nginx; reboot", "../../nginx"] {
        assert!(
            app.validate_operation(&Operation {
                operation: "service-restart".into(),
                confirm: "service-restart".into(),
                service: Some(s.into()),
                version: None
            })
            .is_err()
        );
    }
    assert!(valid_origin("http://public.test", false).is_err());
    assert!(valid_origin("http://public.test", true).is_err());
    assert!(valid_origin("https://a.test/path", false).is_err());
}
#[tokio::test]
async fn http_response_denies_unauthenticated_access_and_never_caches() {
    let (_t, app, _) = sandbox();
    let response = handle(
        State(app),
        Method::GET,
        "/admin/api/config".parse().unwrap(),
        HeaderMap::new(),
        Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("private_key"));
}
