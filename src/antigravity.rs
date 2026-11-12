//! Native Antigravity text generation. Credentials and response bodies are never logged.
mod auth;
mod browser;
#[cfg(test)]
mod tests;
mod wire;

use crate::desktop::i18n::tr;
use color_eyre::eyre::{Result, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

pub(crate) fn login(cancel: &AtomicBool, show_url: Option<&dyn Fn(&str)>) -> Result<()> {
    auth::login(cancel, show_url).map_err(public_error)
}
/// Whether this computer has the client secret (never returns it).
pub(crate) fn client_secret() -> Result<()> {
    auth::secret().map(drop).map_err(public_error)
}
pub(crate) const DEFAULT_MODEL: &str = "gemini-3.8-flash";
pub(crate) fn valid_model_id(id: &str) -> bool {
    wire::usable_model(id)
}
pub(crate) const MODELS: &[&str] = &[
    DEFAULT_MODEL,
    "gemini-3.5-flash",
    "gemini-3.1-pro",
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "gpt-oss-120b",
];
const ENDPOINTS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];
const USER_AGENT: &str = "antigravity/cli/1.4.0 (aidev_client; os_type=linux; arch=amd64; cl=982146307; auth_method=consumer)";
const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Debug)]
struct Status(u16);
impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            401 => f.write_str("Sign in to Antigravity again"),
            429 => f.write_str("Antigravity quota reached. Try again later"),
            code => write!(f, "Antigravity HTTP {code}"),
        }
    }
}
impl std::error::Error for Status {}
fn retryable(error: &color_eyre::eyre::Report) -> bool {
    error
        .downcast_ref::<Status>()
        .is_none_or(|status| matches!(status.0, 403 | 404 | 500 | 502 | 503 | 504))
}
/// A generation moves to the next endpoint only when this one refused it or
/// was never reached. Anything the stream itself said (a block, a length
/// stop, no text) or a broken stream is final: retrying would send the
/// transcript again and spend quota for the same answer.
fn retryable_generation(error: &color_eyre::eyre::Report) -> bool {
    match error.downcast_ref::<Status>() {
        Some(status) => matches!(status.0, 403 | 404 | 500 | 502 | 503 | 504),
        None => error.to_string() == "Antigravity connection failed",
    }
}

fn public_error(error: color_eyre::eyre::Report) -> color_eyre::eyre::Report {
    let message = error.to_string();
    let translated = match message.as_str() {
        "Sign in to Antigravity" => tr("Sign in to Antigravity"),
        "Sign in to Antigravity again" => tr("Sign in to Antigravity again"),
        "Antigravity quota reached. Try again later" => {
            tr("Antigravity quota reached. Try again later")
        }
        "Antigravity request cancelled" => tr("Antigravity request cancelled"),
        "Antigravity request timed out" => tr("Antigravity request timed out"),
        // Sign-in failures that say what to do; none carries a secret.
        "Antigravity sign-in port 51121 is already in use" => {
            tr("Antigravity sign-in port 51121 is already in use")
        }
        "Could not open Google sign-in in your browser" => {
            tr("Could not open Google sign-in in your browser")
        }
        "Antigravity account email unavailable" => tr("Antigravity account email unavailable"),
        "Google sign-in refused" => tr("Google sign-in refused"),
        "Antigravity client secret missing" => tr("Antigravity client secret missing"),
        "Antigravity stopped at its length limit" => tr("Antigravity stopped at its length limit"),
        _ => tr("Antigravity request failed"),
    };
    eyre!("{translated}")
}
static MODEL: RwLock<String> = RwLock::new(String::new());

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    refresh: String,
    access: String,
    expires: u64,
    project_id: String,
    email: String,
}
fn path() -> Result<std::path::PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("antigravity.json"))
}
fn load() -> Result<Credentials> {
    load_at(&path()?)
}
fn load_at(path: &std::path::Path) -> Result<Credentials> {
    let file = std::fs::File::open(path).map_err(|_| eyre!("Sign in to Antigravity"))?;
    serde_json::from_reader(file.take(64 * 1024)).map_err(|_| eyre!("Sign in to Antigravity again"))
}
fn save(c: &Credentials) -> Result<()> {
    save_at(&path()?, c)
}
fn save_at(path: &std::path::Path, c: &Credentials) -> Result<()> {
    crate::app_paths::create_private_dir(path.parent().unwrap())?;
    crate::durable::write_private(path, &serde_json::to_vec(c)?)?;
    Ok(())
}
pub(crate) fn email() -> Option<String> {
    load().ok().map(|c| c.email)
}
pub(crate) fn logout() -> Result<()> {
    let _lock = lock(
        Instant::now() + Duration::from_secs(30),
        &AtomicBool::new(false),
    )?;
    match std::fs::remove_file(path()?) {
        Ok(()) => crate::durable::sync_directory(path()?.parent().unwrap())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    };
    Ok(())
}
pub(crate) fn set_model(model: &str) {
    *MODEL.write().unwrap_or_else(|e| e.into_inner()) = model.into();
}
pub(crate) fn model() -> String {
    let m = MODEL.read().unwrap_or_else(|e| e.into_inner());
    if m.trim().is_empty() {
        DEFAULT_MODEL.into()
    } else {
        m.clone()
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn check(deadline: Instant, cancel: &AtomicBool) -> Result<Duration> {
    if cancel.load(Ordering::Acquire) {
        bail!("Antigravity request cancelled");
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| eyre!("Antigravity request timed out"))
}
fn http(
    url: &str,
    token: Option<&str>,
    body: Option<(&str, &str)>,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    // ureq is blocking. Keep cancellation responsive even during DNS, TLS,
    // headers or a stalled SSE read. An abandoned request has a fixed deadline
    // and cannot persist credentials, retry, or publish an answer.
    use std::sync::{Arc, atomic::AtomicUsize, mpsc};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        }
    }
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    check(deadline, cancel)?;
    ACTIVE
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            // Tests run their fake servers in parallel; the app needs only 4.
            (n < if cfg!(test) { 64 } else { 4 }).then_some(n + 1)
        })
        .map_err(|_| eyre!("Antigravity request busy"))?;
    let permit = Permit;
    let pending = CancelOnDrop(Arc::new(AtomicBool::new(false)));
    let worker_cancel = pending.0.clone();
    let url = url.to_owned();
    let token = token.map(str::to_owned);
    let body = body.map(|(kind, text)| (kind.to_owned(), text.to_owned()));
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("antigravity-http".into())
        .spawn(move || {
            let _permit = permit;
            let result = http_blocking(
                &url,
                token.as_deref(),
                body.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                deadline,
                &worker_cancel,
            );
            let _ = tx.send(result);
        })
        .map_err(|_| eyre!("Antigravity connection failed"))?;
    loop {
        let remaining = check(deadline, cancel)?;
        match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
            Ok(result) => {
                check(deadline, cancel)?;
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("Antigravity connection failed"),
        }
    }
}

fn http_blocking(
    url: &str,
    token: Option<&str>,
    body: Option<(&str, &str)>,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let timeout = check(deadline, cancel)?;
    let agent = crate::net::agent(url, |c| {
        // ureq's body timeout is the whole body's, not each read's, and the
        // answer streams as the body: give both the attempt's own budget.
        c.timeout_global(Some(timeout))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .http_status_as_error(false)
            .max_redirects(0)
    });
    let mut response = if let Some((content_type, data)) = body {
        let mut request = agent
            .post(url)
            .header("Content-Type", content_type)
            .header("User-Agent", USER_AGENT);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request.send(data)
    } else {
        let mut request = agent.get(url).header("User-Agent", USER_AGENT);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request.call()
    }
    .map_err(|_| {
        check(deadline, cancel)
            .err()
            .unwrap_or_else(|| eyre!("Antigravity connection failed"))
    })?;
    check(deadline, cancel)?;
    let status = response.status().as_u16();
    match status {
        200..=299 => (),
        _ => return Err(Status(status).into()),
    }
    let mut reader = response.body_mut().as_reader();
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        check(deadline, cancel)?;
        let n = reader.read(&mut chunk).map_err(|_| {
            check(deadline, cancel)
                .err()
                .unwrap_or_else(|| eyre!("Antigravity response interrupted"))
        })?;
        check(deadline, cancel)?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > LIMIT as usize {
            bail!("Antigravity response too large");
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(bytes)
}
fn post(
    url: &str,
    token: &str,
    body: &Value,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    http(
        url,
        Some(token),
        Some(("application/json", &body.to_string())),
        deadline,
        cancel,
    )
}
fn credentials(deadline: Instant, cancel: &AtomicBool) -> Result<Credentials> {
    credentials_using(&path()?, deadline, cancel, auth::refresh)
}
fn lock(deadline: Instant, cancel: &AtomicBool) -> Result<std::fs::File> {
    lock_at(&path()?, deadline, cancel)
}
fn lock_at(
    path: &std::path::Path,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<std::fs::File> {
    let path = path.with_extension("lock");
    crate::app_paths::create_private_dir(path.parent().unwrap())?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path)?;
    loop {
        check(deadline, cancel)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(e) => return Err(e.into()),
        }
    }
}
fn credentials_using(
    path: &std::path::Path,
    deadline: Instant,
    cancel: &AtomicBool,
    refresh: impl FnOnce(&mut Credentials, Instant, &AtomicBool) -> Result<()>,
) -> Result<Credentials> {
    let _lock = lock_at(path, deadline, cancel)?;
    let mut c = load_at(path)?;
    if c.expires <= now() {
        refresh(&mut c, deadline, cancel)?;
        check(deadline, cancel)?;
        save_at(path, &c)?;
    }
    Ok(c)
}
fn fallback(
    endpoints: &[&str],
    route: &str,
    token: &str,
    body: &Value,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let mut error = eyre!("Antigravity unavailable");
    for endpoint in endpoints {
        check(deadline, cancel)?;
        let seconds = if route.starts_with("streamGenerateContent") {
            30
        } else {
            8
        };
        let attempt = (Instant::now() + Duration::from_secs(seconds)).min(deadline);
        match post(
            &format!("{endpoint}/v1internal:{route}"),
            token,
            body,
            attempt,
            cancel,
        ) {
            Ok(data) => return Ok(data),
            Err(e) => {
                if !retryable(&e) {
                    return Err(e);
                }
                error = e;
            }
        }
    }
    Err(error)
}
pub(crate) fn generate(
    model: &str,
    instructions: &str,
    prompt: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let result = (|| {
        if !wire::usable_model(model)
            || model.len() > 256
            || instructions.len() + prompt.len() > LIMIT as usize
        {
            bail!("Invalid Antigravity request");
        }
        let c = credentials(deadline, cancel)?;
        generate_at(ENDPOINTS, &c, model, instructions, prompt, deadline, cancel)
    })();
    result.map_err(public_error)
}
fn generate_at(
    endpoints: &[&str],
    c: &Credentials,
    model: &str,
    instructions: &str,
    prompt: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String> {
    let mut error = eyre!("Antigravity unavailable");
    for endpoint in endpoints {
        check(deadline, cancel)?;
        // Retries need fresh identities: the previous session may still be
        // generating on the backend after a broken connection.
        let body = wire::envelope(&c.project_id, model, instructions, prompt)?;
        let attempt = (Instant::now() + Duration::from_secs(90)).min(deadline);
        let result = post(
            &format!("{endpoint}/v1internal:streamGenerateContent?alt=sse"),
            &c.access,
            &body,
            attempt,
            cancel,
        )
        .and_then(|bytes| wire::parse_sse(&bytes));
        match result {
            Ok(answer) => return Ok(answer),
            Err(e) if !retryable_generation(&e) => {
                return Err(e);
            }
            Err(e) => error = e,
        }
    }
    check(deadline, cancel)?;
    Err(error)
}
pub(crate) fn models(cancel: &AtomicBool) -> Result<Vec<String>> {
    let deadline = Instant::now() + Duration::from_secs(25);
    let c = credentials(deadline, cancel)?;
    models_at(ENDPOINTS, &c, deadline, cancel)
}
fn models_at(
    endpoints: &[&str],
    c: &Credentials,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<String>> {
    let bytes = fallback(
        endpoints,
        "fetchAvailableModels",
        &c.access,
        &json!({"project":c.project_id}),
        deadline,
        cancel,
    )?;
    let data: Value =
        serde_json::from_slice(&bytes).map_err(|_| eyre!("Invalid Antigravity model catalog"))?;
    let models = data["models"]
        .as_object()
        .ok_or_else(|| eyre!("Antigravity model catalog unavailable"))?;
    wire::remember_models(models);
    Ok(models
        .keys()
        .filter(|id| wire::usable_model(id))
        .take(256)
        .cloned()
        .collect())
}
