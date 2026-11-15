use super::*;

#[test]
fn remote_bind_and_peer_filter_refuse_other_interfaces() {
    for ip in [
        "0.0.0.0",
        "127.0.0.1",
        "192.168.0.2",
        "8.8.8.8",
        "100.63.255.255",
        "100.128.0.0",
        "::1",
        "::ffff:100.64.0.1",
    ] {
        let ip = ip.parse().unwrap();
        assert!(bind_address(ip, 47821).is_err(), "{ip}");
        assert!(!peer_allowed(ip), "{ip}");
    }
    for ip in ["100.64.0.0", "100.127.255.255"] {
        assert!(bind_address(ip.parse().unwrap(), 47821).is_ok());
    }
    assert!(peer_allowed("fd7a:115c:a1e0::1".parse().unwrap()));
    assert!(!peer_allowed("fd7a:115c:a1e1::1".parse().unwrap()));
    assert!(bind_address("100.64.0.1".parse().unwrap(), 0).is_err());
}

#[test]
fn remote_routes_are_an_exact_allowlist() {
    for (method, path) in [
        ("GET", "/health"),
        ("GET", "/capabilities"),
        ("POST", "/pair"),
        ("POST", "/transcriptions"),
        ("POST", "/streams"),
        ("GET", "/streams/1/events"),
        ("POST", "/streams/1/audio"),
        ("POST", "/streams/1/finish"),
        ("POST", "/streams/1/cancel"),
        ("POST", "/streams/1/heartbeat"),
    ] {
        assert!(allowed(method, path));
    }
    for path in [
        "/control",
        "/dev/control",
        "/dictations",
        "/meetings/events",
        "/models",
        "/models/a/prepare",
        "/profile/vitals",
        "/streams/1/extra/audio",
        "/streams/1/control",
        "/streams/1/events/",
        "/streams-evil",
        "/pair/devices",
        "/%63ontrol",
    ] {
        for method in ["GET", "POST", "DELETE", "OPTIONS"] {
            assert!(!allowed(method, path), "{method} {path}");
        }
    }
    assert!(!allowed("GET", "/pair"));
    assert!(!allowed("GET", "/transcriptions"));
}

#[test]
fn remote_lab_is_off_by_default_and_ships_to_everyone() {
    use crate::labs::{Audience, Feature, Group};
    let mut settings = crate::app_settings::AppSettings::default();
    assert!(!Feature::RemoteDictation.on(&settings));
    assert_eq!(settings.remote_dictation.port, 47821);
    assert!(!settings.remote_dictation.process_transcripts);
    assert_eq!(Feature::RemoteDictation.audience(), Audience::Everyone);
    assert_eq!(Feature::RemoteDictation.group(), Group::Dictation);
    Feature::RemoteDictation.set(&mut settings, true);
    assert!(Feature::RemoteDictation.on(&settings));
    Feature::RemoteDictation.set(&mut settings, false);
    assert!(!Feature::RemoteDictation.on(&settings));
}

#[test]
fn remote_http_gate_auth_revocation_and_pairing_limits() {
    let directory =
        std::env::temp_dir().join(format!("hex-remote-routes-{}", generate_token().unwrap()));
    std::fs::create_dir_all(&directory).unwrap();
    let (mut service, transcription) =
        crate::transcription::service::TranscriptionService::start().unwrap();
    let context = HttpContext {
        token: "loopback-token".into(),
        remote: None,
        health: Arc::new(current_health),
        events: EventLog::create(&directory.join("events.ndjson")).unwrap(),
        auth_limiter: AuthObservationLimiter::default(),
        model_preparing: Arc::default(),
        shutdown: Arc::default(),
        transcription,
        developer_control: None,
        control: None,
        dictations: dictation::Dictations::new(None),
        streams: Arc::default(),
        stream_engine: Arc::new(crate::transcription::stream::shared_engine),
        meeting_events: meeting_events::Source::default(),
        processing: Processing::default(),
        timestamps: Timestamps::default(),
    };
    let state = Arc::new(State::new());
    let mut devices = Devices::load(directory.join("devices.json")).unwrap();
    let now = Instant::now();
    let code = devices.begin_pair(now).unwrap();
    let (id, token) = devices
        .pair(
            PairRequest {
                code,
                name: "Phone".into(),
            },
            now,
        )
        .unwrap();
    *state.devices.lock().unwrap() = Some(devices);
    let call = |method: &str, path: &str, token: &str, length: usize, enabled: bool| {
        let request = HttpRequest {
            method: method.into(),
            path: path.into(),
            authorization: Some(format!("Bearer {token}")),
            dictation_token: None,
            stream_token: None,
            content_type: Some("application/json".into()),
            content_length: Some(length),
            body_prefix: Vec::new(),
        };
        let mut observation = None;
        let action =
            request_under_gate(request, &context, state.clone(), &mut observation, enabled);
        match action {
            RequestAction::Respond(response) => response,
            _ => panic!("expected an immediate response"),
        }
    };
    assert_eq!(call("GET", "/health", &token, 0, false).status, 503);
    assert_eq!(call("POST", "/pair", "", 0, false).status, 503);
    assert_eq!(call("POST", "/control", "", 0, true).status, 404);
    assert_eq!(
        call("GET", "/health", "loopback-token", 0, true).status,
        401
    );
    assert_eq!(call("GET", "/health", &token, 0, true).status, 200);
    let caps = call("GET", "/capabilities", &token, 0, true);
    let caps: serde_json::Value = serde_json::from_slice(&caps.body).unwrap();
    assert_eq!(caps["control"], false);
    assert_eq!(caps["developerControl"], false);
    assert_eq!(caps["serviceCapture"], false);
    assert_eq!(
        call("POST", "/pair", "", MAX_PAIR_BYTES + 1, true).status,
        413
    );
    assert_eq!(call("POST", "/streams", &token, 1025, true).status, 413);
    state
        .devices
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .revoke(&id)
        .unwrap();
    assert_eq!(call("GET", "/health", &token, 0, true).status, 401);
    drop(context);
    service.shutdown();
    std::fs::remove_dir_all(directory).unwrap();
}
