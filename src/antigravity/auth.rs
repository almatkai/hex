//! Browser OAuth with PKCE and a bounded, cancellable loopback callback.
use super::*;
use std::io::Write;
use std::net::TcpListener;
const REDIRECT: &str = "http://localhost:51121/oauth-callback";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
/// The desktop client's secret is kept out of the repository: it comes from
/// `ANTIGRAVITY_CLIENT_SECRET`, else `antigravity-client-secret.txt` in Hex's
/// data folder, filled in once per computer.
pub(super) const SECRET_FILE: &str = "antigravity-client-secret.txt";
pub(super) fn secret() -> Result<String> {
    if let Ok(value) = std::env::var("ANTIGRAVITY_CLIENT_SECRET")
        && !value.trim().is_empty()
    {
        return Ok(value.trim().to_owned());
    }
    let path = crate::app_paths::support_dir()?.join(SECRET_FILE);
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(1024).read_to_string(&mut text))
        .ok()
        .filter(|_| !text.trim().is_empty())
        .map(|_| text.trim().to_owned())
        .ok_or_else(|| eyre!("Antigravity client secret missing"))
}
const SCOPES: &[&str] = &[
    "aicode",
    "cloud-platform",
    "userinfo.email",
    "userinfo.profile",
    "cclog",
    "experimentsandconfigs",
];
fn form(fields: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish()
}
fn token(fields: &[(&str, &str)], deadline: Instant, cancel: &AtomicBool) -> Result<Value> {
    token_at(TOKEN, fields, deadline, cancel)
}
pub(super) fn token_at(
    url: &str,
    fields: &[(&str, &str)],
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Value> {
    let secret = if cfg!(test) {
        "test-secret".to_owned()
    } else {
        secret()?
    };
    let mut fields = fields.to_vec();
    fields.extend([("client_id", ID), ("client_secret", secret.as_str())]);
    let bytes = http(
        url,
        None,
        Some(("application/x-www-form-urlencoded", &form(&fields))),
        deadline,
        cancel,
    )
    .map_err(|e| {
        if e.downcast_ref::<Status>()
            .is_some_and(|s| matches!(s.0, 400 | 401))
        {
            eyre!("Sign in to Antigravity again")
        } else {
            e
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| eyre!("Invalid Antigravity token response"))
}
pub(super) fn apply(c: &mut Credentials, data: &Value) -> Result<()> {
    c.access = data["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| eyre!("Sign in to Antigravity again"))?
        .into();
    if let Some(refresh) = data["refresh_token"].as_str() {
        c.refresh = refresh.into();
    }
    if c.refresh.is_empty() {
        bail!("Sign in to Antigravity again");
    }
    c.expires = now().saturating_add(
        data["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .saturating_sub(300)
            .saturating_mul(1000),
    );
    Ok(())
}
pub(super) fn refresh(c: &mut Credentials, deadline: Instant, cancel: &AtomicBool) -> Result<()> {
    let data = token(
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &c.refresh),
        ],
        deadline,
        cancel,
    )?;
    apply(c, &data)
}
/// `show_url` also gets the sign-in address (it carries the state and the
/// PKCE challenge, never the verifier), so a terminal can print it for a
/// browser Hex couldn't open; then a failed open isn't fatal.
pub(crate) fn login(cancel: &AtomicBool, show_url: Option<&dyn Fn(&str)>) -> Result<()> {
    check(Instant::now() + Duration::from_secs(300), cancel)?;
    // Fail before the browser opens, not after the person signed in.
    secret()?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 51121))
        .map_err(|_| eyre!("Antigravity sign-in port 51121 is already in use"))?;
    let verifier = wire::base64(&wire::random()?);
    let state = wire::base64(&wire::random()?);
    let url = authorization_url(&verifier, &state);
    if let Some(show_url) = show_url {
        show_url(&url);
        let _ = super::browser::open(&url);
    } else {
        super::browser::open(&url)?;
    }
    let code = callback(
        listener,
        &state,
        Instant::now() + Duration::from_secs(300),
        cancel,
    )?;
    finish_login(&code, &verifier, cancel)
}

pub(super) fn authorization_url(verifier: &str, state: &str) -> String {
    let scopes = SCOPES
        .iter()
        .map(|s| format!("https://www.googleapis.com/auth/{s}"))
        .collect::<Vec<_>>()
        .join(" ");
    let query = form(&[
        ("client_id", ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT),
        ("scope", &scopes),
        ("code_challenge", &wire::challenge(verifier)),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ]);
    format!("https://accounts.google.com/o/oauth2/v2/auth?{query}")
}

fn finish_login(code: &str, verifier: &str, cancel: &AtomicBool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let data = token(
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT),
            ("code_verifier", verifier),
        ],
        deadline,
        cancel,
    )?;
    let mut c = Credentials {
        refresh: String::new(),
        access: String::new(),
        expires: 0,
        project_id: String::new(),
        email: String::new(),
    };
    apply(&mut c, &data)?;
    let info = http(
        "https://www.googleapis.com/oauth2/v1/userinfo?alt=json",
        Some(&c.access),
        None,
        deadline,
        cancel,
    )?;
    c.email = serde_json::from_slice::<Value>(&info)
        .ok()
        .and_then(|v| v["email"].as_str().map(str::to_owned))
        .ok_or_else(|| eyre!("Antigravity account email unavailable"))?;
    c.project_id = discover(ENDPOINTS, &c, deadline, cancel)?;
    check(deadline, cancel)?;
    let _lock = lock(deadline, cancel)?;
    check(deadline, cancel)?;
    save(&c)
}

pub(super) fn discover(
    endpoints: &[&str],
    c: &Credentials,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String> {
    for (route, body) in [
        (
            "loadCodeAssist",
            json!({"metadata":{"ideType":"ANTIGRAVITY"}}),
        ),
        ("listCloudAICompanionProjects", json!({})),
    ] {
        if let Ok(bytes) = fallback(endpoints, route, &c.access, &body, deadline, cancel)
            && let Ok(v) = serde_json::from_slice::<Value>(&bytes)
            && let Some(project) = project(&v)
        {
            return Ok(project);
        }
    }
    check(deadline, cancel)?;
    Ok(wire::stable_project(&c.email))
}
fn project(v: &Value) -> Option<String> {
    for key in [
        "antigravityProjectId",
        "projectId",
        "backendProjectId",
        "userDefinedCloudaicompanionProject",
        "cloudaicompanionProject",
        "project",
    ] {
        if let Some(s) = v[key]
            .as_str()
            .or_else(|| v[key]["id"].as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(s.into());
        }
    }
    for key in ["projects", "projectIds", "cloudaicompanionProjects"] {
        if let Some(a) = v[key].as_array() {
            for v in a {
                if let Some(s) = v.as_str().map(str::to_owned).or_else(|| project(v)) {
                    return Some(s);
                }
            }
        }
    }
    None
}
pub(super) fn callback(
    listener: TcpListener,
    state: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String> {
    listener.set_nonblocking(true)?;
    loop {
        check(deadline, cancel)?;
        match listener.accept() {
            Ok((mut stream, _)) => {
                // macOS and Windows hand out non-blocking sockets from a
                // non-blocking listener; the read timeout needs a blocking one.
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                stream.set_write_timeout(Some(Duration::from_millis(100)))?;
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                let read_deadline = (Instant::now() + Duration::from_secs(2)).min(deadline);
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    check(deadline, cancel)?;
                    if Instant::now() >= read_deadline {
                        break;
                    }
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) =>
                        {
                            continue;
                        }
                        Err(_) => break,
                    }
                    if bytes.len() > 8192 {
                        break;
                    }
                }
                let result = if bytes.len() > 8192 || !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    Err(eyre!("Invalid OAuth callback"))
                } else {
                    callback_request(&bytes, state)
                };
                let message = if result.is_ok() {
                    tr("Sign-in complete. Return to Hex.")
                } else {
                    tr("Sign-in failed. Return to Hex.")
                };
                let status = if result.is_ok() {
                    "200 OK"
                } else {
                    "400 Bad Request"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{message}",
                    message.len()
                );
                // Browser probes (favicon, speculative connections) aren't a
                // callback. A real callback, including a bad state, settles it.
                if bytes.starts_with(b"GET /oauth-callback?") {
                    return result;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(e) => return Err(e.into()),
        }
    }
}
pub(super) fn callback_request(bytes: &[u8], state: &str) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|_| eyre!("Invalid OAuth callback"))?;
    let mut parts = text.lines().next().unwrap_or("").split_whitespace();
    if parts.next() != Some("GET") {
        bail!("Invalid OAuth callback method");
    }
    let target = parts.next().unwrap_or("");
    if !target.starts_with("/oauth-callback?") {
        bail!("Invalid OAuth callback path");
    }
    let url = url::Url::parse(&format!("http://localhost{target}"))?;
    let q: std::collections::HashMap<_, _> = url.query_pairs().collect();
    if q.get("state").map(|v| v.as_ref()) != Some(state) {
        bail!("OAuth state mismatch");
    }
    if q.contains_key("error") {
        bail!("Google sign-in refused");
    }
    q.get("code")
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .ok_or_else(|| eyre!("Missing OAuth code"))
}
