use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
fn fake(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut bytes = Vec::new();
        let mut b = [0; 1024];
        loop {
            let n = s.read(&mut b).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&b[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..end]);
                let size = header
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + size {
                    break;
                }
            }
        }
        write!(
            s,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        // Deliberately split UTF-8 and SSE framing over transport reads.
        for b in body.as_bytes().chunks(3) {
            s.write_all(b).unwrap();
        }
        String::from_utf8(bytes).unwrap()
    });
    (url, worker)
}
#[test]
fn envelope_is_text_only_and_has_cli_identity() {
    let v = wire::envelope("project", "gemini-3.8-flash", "system", "transcript").unwrap();
    assert_eq!(v["project"], "project");
    assert_eq!(v["model"], "gemini-3.8-flash-low");
    assert_eq!(v["requestType"], "agent");
    assert_eq!(v["userAgent"], "antigravity");
    assert_eq!(
        v["request"]["contents"],
        json!([{"role":"user","parts":[{"text":"transcript"}]}])
    );
    assert_eq!(
        v["request"]["systemInstruction"],
        json!({"role":"user","parts":[{"text":"system"}]})
    );
    assert_eq!(v["request"]["labels"]["last_step_index"], "0");
    assert_eq!(
        v["request"]["labels"]["model_enum"],
        "MODEL_PLACEHOLDER_M320"
    );
    assert!(v["request"]["tools"].is_null());
    assert!(v["requestId"].as_str().unwrap().starts_with("agent/"));
    assert!(
        v["request"]["sessionId"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .is_ok()
    );
}
const SSE: &str = "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hé\"}]}}]}}\r\n\r\ndata: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"hidden\"},{\"text\":\"llo\"}]},\"finishReason\":\"STOP\"}]}}\n\ndata: [DONE]\n\n";
#[test]
fn fake_stream_falls_back_and_parses_split_utf8_sse() {
    let (bad, a) = fake(503, "");
    let (good, b) = fake(200, SSE);
    let bytes = fallback(
        &[&bad, &good],
        "streamGenerateContent?alt=sse",
        "secret",
        &json!({"project":"p"}),
        Instant::now() + Duration::from_secs(10),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(wire::parse_sse(&bytes).unwrap(), "héllo");
    a.join().unwrap();
    let request = b.join().unwrap();
    assert!(request.contains("/v1internal:streamGenerateContent?alt=sse"));
    assert!(request.contains(USER_AGENT));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer secret")
    );
}
#[test]
fn refresh_uses_form_and_preserves_refresh_token() {
    let (url, worker) = fake(200, r#"{"access_token":"new-access","expires_in":3600}"#);
    let data = auth::token_at(
        &url,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", "old-refresh"),
        ],
        Instant::now() + Duration::from_secs(10),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut c = Credentials {
        refresh: "old-refresh".into(),
        access: "old".into(),
        expires: 0,
        project_id: "p".into(),
        email: "e".into(),
    };
    auth::apply(&mut c, &data).unwrap();
    assert_eq!(c.refresh, "old-refresh");
    assert_eq!(c.access, "new-access");
    assert!(c.expires > now());
    assert_eq!(c.project_id, "p");
    let request = worker.join().unwrap();
    assert!(request.contains("grant_type=refresh_token"));
    assert!(request.contains("refresh_token=old-refresh"));
}
#[test]
fn callback_checks_state_timeout_and_cancellation() {
    assert!(
        auth::callback_request(
            b"GET /oauth-callback?state=wrong&code=secret HTTP/1.1\r\n\r\n",
            "expected"
        )
        .unwrap_err()
        .to_string()
        .contains("state mismatch")
    );
    assert_eq!(
        auth::callback_request(
            b"GET /oauth-callback?state=expected&code=a%2Bb HTTP/1.1\r\n\r\n",
            "expected"
        )
        .unwrap(),
        "a+b"
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(
        auth::callback(
            listener,
            "s",
            Instant::now() + Duration::from_millis(35),
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .to_string()
        .contains("timed out")
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(
        auth::callback(
            listener,
            "s",
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(true)
        )
        .unwrap_err()
        .to_string()
        .contains("cancelled")
    );
}
#[test]
fn private_credential_write_is_atomic_and_owner_only() {
    let root = std::env::temp_dir().join(format!("hex-antigravity-{}", now()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("credentials.json");
    crate::durable::write_private(&path, b"first").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    }
    crate::durable::write_private(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn pkce_and_project_match_reference_vectors() {
    assert_eq!(
        wire::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(
        wire::stable_project("test@example.com"),
        "a842d62a-c134-502c-8a15-c6208da4e375"
    );
}
#[test]
fn streams_reject_errors_empty_and_truncated_answers() {
    assert!(wire::parse_sse(b"data: {bad}\n\n").is_err());
    assert!(wire::parse_sse(b"data: {\"error\":{}}\n\n").is_err());
    assert!(wire::parse_sse(b"data: [DONE]\n\n").is_err());
    assert!(wire::parse_sse(b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"partial private text\"}]}}]}\n\n").is_err());
    let error = wire::parse_sse(
        b"data: {\"error\":{\"code\":429,\"message\":\"private provider body\"}}\n\n",
    )
    .unwrap_err();
    assert!(error.to_string().contains("quota"));
    assert!(!error.to_string().contains("private"));
}

fn credential() -> Credentials {
    Credentials {
        refresh: "refresh".into(),
        access: "access".into(),
        expires: 0,
        project_id: "project".into(),
        email: "test@example.com".into(),
    }
}

#[test]
fn an_answer_the_stream_refused_is_final_and_never_resent() {
    for body in [
        "data: {\"error\":{\"code\":400,\"message\":\"blocked\"}}\n\n",
        "data: {\"response\":{\"candidates\":[{\"finishReason\":\"MAX_TOKENS\",\"content\":{\"parts\":[{\"text\":\"cut\"}]}}]}}\n\n",
    ] {
        let (first, _served) = fake(200, body);
        // Never contacted: a retry would send the transcript a second time.
        let unreachable = "http://127.0.0.1:9";
        let error = generate_at(
            &[&first, unreachable],
            &credential(),
            DEFAULT_MODEL,
            "system",
            "transcript",
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_ne!(error.to_string(), "Antigravity connection failed");
    }
    assert!(!retryable_generation(&eyre!(
        "Antigravity returned no text"
    )));
    assert!(!retryable_generation(&eyre!(
        "Antigravity response interrupted"
    )));
    assert!(retryable_generation(&eyre!(
        "Antigravity connection failed"
    )));
    assert!(retryable_generation(&Status(503).into()));
    assert!(!retryable_generation(&Status(429).into()));
}

#[test]
fn generation_retries_under_fresh_identity_and_sends_the_envelope() {
    let (bad, a) = fake(503, "private error body");
    let (good, b) = fake(200, SSE);
    let answer = generate_at(
        &[&bad, &good],
        &credential(),
        DEFAULT_MODEL,
        "system",
        "transcript",
        Instant::now() + Duration::from_secs(5),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(answer, "héllo");
    let requests: Vec<Value> = [a, b]
        .into_iter()
        .map(|thread| {
            let raw = thread.join().unwrap();
            serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap()
        })
        .collect();
    assert_ne!(requests[0]["requestId"], requests[1]["requestId"]);
    assert_ne!(
        requests[0]["request"]["sessionId"],
        requests[1]["request"]["sessionId"]
    );
    assert_eq!(
        requests[1]["request"]["contents"][0]["parts"][0]["text"],
        "transcript"
    );
    assert_eq!(requests[1]["project"], "project");
}

#[test]
fn public_models_use_the_references_reasoning_off_routes() {
    for (id, route) in [
        (DEFAULT_MODEL, "gemini-3.8-flash-low"),
        ("gemini-3.5-flash", "gemini-3.5-flash-extra-low"),
        ("gemini-3.1-pro", "gemini-3.1-pro-low"),
        ("claude-sonnet-4-6", "claude-sonnet-4-6"),
        ("claude-opus-4-6", "claude-opus-4-6-thinking"),
        ("gpt-oss-120b", "gpt-oss-120b-medium"),
    ] {
        let envelope = wire::envelope("p", id, "s", "u").unwrap();
        assert_eq!(envelope["model"], route);
        assert_eq!(
            envelope["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
    }
    for id in [
        "MODEL_PLACEHOLDER_M320",
        "chat_internal",
        "tab_internal",
        "gemini-image",
        "bad id",
        "",
    ] {
        assert!(!wire::usable_model(id));
    }
    assert!(wire::usable_model("gemini-4-argon"));
}

#[test]
fn an_expired_token_is_refreshed_once_and_persisted_privately() {
    let root =
        std::env::temp_dir().join(format!("hex-ag-refresh-{}-{}", std::process::id(), now()));
    let path = root.join("antigravity.json");
    save_at(&path, &credential()).unwrap();
    let (url, server) = fake(200, r#"{"access_token":"refreshed","expires_in":3600}"#);
    let cancel = AtomicBool::new(false);
    let c = credentials_using(
        &path,
        Instant::now() + Duration::from_secs(5),
        &cancel,
        |c, deadline, cancel| {
            let data = auth::token_at(
                &url,
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &c.refresh),
                ],
                deadline,
                cancel,
            )?;
            auth::apply(c, &data)
        },
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(c.access, "refreshed");
    assert_eq!(load_at(&path).unwrap().access, "refreshed");
    let c = credentials_using(
        &path,
        Instant::now() + Duration::from_secs(1),
        &cancel,
        |_, _, _| panic!("a fresh token is reused"),
    )
    .unwrap();
    assert_eq!(c.project_id, "project");
    assert_eq!(c.refresh, "refresh");
    assert_eq!(c.email, "test@example.com");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_cancelled_refresh_never_replaces_credentials() {
    let root = std::env::temp_dir().join(format!("hex-ag-cancel-{}-{}", std::process::id(), now()));
    let path = root.join("antigravity.json");
    save_at(&path, &credential()).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        credentials_using(
            &path,
            Instant::now() + Duration::from_secs(1),
            &cancel,
            |c, _, cancel| {
                c.access = "never saved".into();
                cancel.store(true, Ordering::Release);
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(load_at(&path).unwrap().access, "access");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancellation_interrupts_waiting_for_http_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let trigger = cancel.clone();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        trigger.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(250));
    });
    let start = Instant::now();
    let error = http(&url, None, None, start + Duration::from_secs(5), &cancel).unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}

#[test]
fn http_deadline_and_body_limit_are_enforced() {
    let (url, server) = fake(200, SSE);
    assert!(
        http(
            &url,
            None,
            None,
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(false)
        )
        .is_ok()
    );
    server.join().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0; 1024];
        stream.read(&mut buffer).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            LIMIT + 1
        )
        .unwrap();
        let _ = stream.write_all(&vec![b'x'; LIMIT as usize + 1]);
    });
    assert!(
        http(
            &url,
            None,
            None,
            Instant::now() + Duration::from_secs(5),
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .to_string()
        .contains("too large")
    );
    server.join().unwrap();
    assert!(
        http(
            "http://127.0.0.1:1",
            None,
            None,
            Instant::now(),
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .to_string()
        .contains("timed out")
    );
}

#[test]
fn callback_listener_rejects_a_mismatched_state() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        auth::callback(
            listener,
            "expected",
            Instant::now() + Duration::from_secs(3),
            &AtomicBool::new(false),
        )
    });
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(b"GET /oauth-callback?state=wrong&code=secret HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut reply = String::new();
    s.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 400"));
    assert!(!reply.contains("secret"));
    assert!(
        server
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("state mismatch")
    );
}
#[test]
fn auth_and_quota_errors_do_not_retry_another_endpoint() {
    for (status, message) in [(401, "Sign in to Antigravity again"), (429, "quota")] {
        let (url, server) = fake(status, "");
        let error = fallback(
            &[&url, "http://127.0.0.1:1"],
            "test",
            "token",
            &json!({}),
            Instant::now() + Duration::from_secs(3),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains(message));
        server.join().unwrap();
    }
}

#[test]
fn oauth_url_has_independent_state_pkce_and_all_reference_scopes() {
    let url = url::Url::parse(&auth::authorization_url("verifier", "independent-state")).unwrap();
    let fields: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    assert_eq!(
        fields["redirect_uri"],
        "http://localhost:51121/oauth-callback"
    );
    assert_eq!(fields["state"], "independent-state");
    assert_eq!(fields["code_challenge"], wire::challenge("verifier"));
    assert_eq!(fields["code_challenge_method"], "S256");
    assert_eq!(fields["access_type"], "offline");
    assert_eq!(fields["prompt"], "consent");
    assert_eq!(fields["scope"].split_whitespace().count(), 6);
    assert!(fields["scope"].contains("https://www.googleapis.com/auth/aicode"));
    assert!(!url.as_str().contains("code_verifier"));
}

#[test]
fn project_discovery_uses_load_then_list_then_stable_email() {
    let (first, load) = fake(200, "{}");
    let (second, list) = fake(200, r#"{"projects":[{"projectId":"discovered"}]}"#);
    let c = credential();
    let cancel = AtomicBool::new(false);
    let project = auth::discover(
        &[&first, &second],
        &c,
        Instant::now() + Duration::from_secs(5),
        &cancel,
    )
    .unwrap();
    assert_eq!(project, "discovered");
    let request = load.join().unwrap();
    assert!(request.contains("/v1internal:loadCodeAssist"));
    let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body, json!({"metadata":{"ideType":"ANTIGRAVITY"}}));
    assert!(
        list.join()
            .unwrap()
            .contains("/v1internal:listCloudAICompanionProjects")
    );
    assert_eq!(
        auth::discover(&[], &c, Instant::now() + Duration::from_secs(1), &cancel).unwrap(),
        wire::stable_project(&c.email)
    );
}

#[test]
fn callback_accepts_valid_state_and_releases_the_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        auth::callback(
            listener,
            "expected",
            Instant::now() + Duration::from_secs(3),
            &AtomicBool::new(false),
        )
    });
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .write_all(
            b"GET /oauth-callback?code=secret&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(!response.contains("secret"));
    assert_eq!(server.join().unwrap().unwrap(), "secret");
    assert!(std::net::TcpStream::connect(addr).is_err());
}

#[test]
fn credential_lock_waits_are_bounded_and_cancellable() {
    let root = std::env::temp_dir().join(format!("hex-ag-lock-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("antigravity.json");
    let held = lock_at(
        &path,
        Instant::now() + Duration::from_secs(1),
        &AtomicBool::new(false),
    )
    .unwrap();
    let error = lock_at(
        &path,
        Instant::now() + Duration::from_millis(35),
        &AtomicBool::new(false),
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(
        lock_at(
            &path,
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(true)
        )
        .is_err()
    );
    drop(held);
    drop(
        lock_at(
            &path,
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .unwrap(),
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_model_catalog_uses_runtime_keys_and_supplies_envelope_labels() {
    let (url, server) = fake(
        200,
        r#"{"models":{"gemini-test":{"model":"MODEL_TEST"},"MODEL_INTERNAL":{},"gemini-image":{}}}"#,
    );
    let models = models_at(
        &[&url],
        &credential(),
        Instant::now() + Duration::from_secs(3),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(models, ["gemini-test"]);
    let request = server.join().unwrap();
    assert!(request.contains("/v1internal:fetchAvailableModels"));
    assert_eq!(
        serde_json::from_str::<Value>(request.split_once("\r\n\r\n").unwrap().1).unwrap(),
        json!({"project":"project"})
    );
    let envelope = wire::envelope("project", "gemini-test", "s", "u").unwrap();
    assert_eq!(envelope["model"], "gemini-test");
    assert_eq!(envelope["request"]["labels"]["model_enum"], "MODEL_TEST");
}
