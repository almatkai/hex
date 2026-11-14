//! A separate, default-off tailnet boundary over the existing API handlers.
//! Never publish the loopback token, control routes, or a wildcard listener.
mod devices;
mod network;
#[cfg(test)]
mod tests;

use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};

use super::*;
pub(crate) use devices::Device;
use devices::{Devices, MAX_PAIR_BYTES, PairRequest};

static CONFIG: LazyLock<RwLock<crate::app_settings::RemoteDictation>> =
    LazyLock::new(Default::default);
static CURRENT: Mutex<Weak<State>> = Mutex::new(Weak::new());

pub(crate) fn configure(settings: &crate::app_settings::AppSettings) {
    let mut config = settings.remote_dictation.clone();
    config.enabled = crate::labs::Feature::RemoteDictation.on(settings)
        && crate::labs::Feature::RemoteDictation.shipped()
        && crate::labs::Feature::RemoteDictation.on_this_os()
        && crate::labs::Feature::RemoteDictation.managed().is_none();
    *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = config;
}

pub(crate) fn enabled() -> bool {
    CONFIG.read().unwrap_or_else(|e| e.into_inner()).enabled
}
fn processing_enabled() -> bool {
    let config = CONFIG.read().unwrap_or_else(|e| e.into_inner());
    config.enabled && config.process_transcripts
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, clap::Subcommand)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Command {
    /// Print a one-use pairing payload, valid for five minutes.
    Pair,
    /// List paired devices and the listener address.
    Devices,
    /// Remove a device's access.
    Revoke { id: String },
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub address: Option<String>,
    pub status: String,
    pub devices: Vec<Device>,
}

pub(super) struct State {
    snapshot: Mutex<Snapshot>,
    devices: Mutex<Option<Devices>>,
    shutdown: Mutex<Weak<AtomicBool>>,
    streams: Mutex<Vec<Weak<crate::transcription::stream::Stream>>>,
}

impl State {
    fn new() -> Self {
        Self {
            snapshot: Mutex::default(),
            devices: Mutex::default(),
            shutdown: Mutex::default(),
            streams: Mutex::default(),
        }
    }
    pub(super) fn track(&self, stream: &Arc<crate::transcription::stream::Stream>) {
        let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        streams.retain(|s| s.strong_count() > 0);
        streams.push(Arc::downgrade(stream));
    }
    fn cancel(&self) {
        if let Some(shutdown) = self
            .shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .upgrade()
        {
            shutdown.store(true, Ordering::Release);
        }
        for stream in self
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .filter_map(|s| s.upgrade())
        {
            stream.cancel();
        }
    }
    fn status(&self, status: &str, address: Option<SocketAddr>) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        snapshot.status = status.into();
        snapshot.address = address.map(|a| format!("http://{a}"));
    }
}

pub(crate) fn snapshot() -> Snapshot {
    let Some(state) = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).upgrade() else {
        return Snapshot {
            status: "Remote listener unavailable".into(),
            ..Default::default()
        };
    };
    let mut snapshot = state
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    snapshot.devices = state
        .devices
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(Devices::list)
        .unwrap_or_default();
    snapshot
}

/// Runs only inside the app; the CLI sends this over authenticated loopback control.
pub(crate) fn command(command: Command) -> Result<String> {
    if !enabled() {
        return Err(eyre!("Turn on Remote dictation in Labs"));
    }
    let state = CURRENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .upgrade()
        .ok_or_else(|| eyre!("Remote listener unavailable"))?;
    let mut devices = state.devices.lock().unwrap_or_else(|e| e.into_inner());
    let devices = devices
        .as_mut()
        .ok_or_else(|| eyre!("Remote device store unavailable"))?;
    match command {
        Command::Pair => {
            let address = state
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .address
                .clone()
                .ok_or_else(|| eyre!("No Tailscale listener address"))?;
            let code = devices.begin_pair(Instant::now())?;
            Ok(serde_json::to_string(
                &serde_json::json!({"type":"hex-remote-pair", "version":1, "url":address, "code":code, "expiresInSeconds":300}),
            )?)
        }
        Command::Devices => {
            let mut snapshot = state
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            snapshot.devices = devices.list();
            Ok(serde_json::to_string_pretty(&snapshot)?)
        }
        Command::Revoke { id } => {
            let changed = devices.revoke(&id)?;
            if changed {
                state.cancel();
            }
            Ok(if changed {
                "Device revoked"
            } else {
                "Device already absent"
            }
            .into())
        }
    }
}

pub(super) struct Supervisor {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Supervisor {
    pub(super) fn start(context: &HttpContext, local_port: u16) -> Result<Self> {
        let state = Arc::new(State::new());
        *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(&state);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let context = remote_context(context);
        let worker = thread::Builder::new()
            .name("remote-dictation".into())
            .spawn(move || {
                supervise(context, state, stopped, local_port);
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn remote_context(local: &HttpContext) -> HttpContext {
    HttpContext {
        token: "".into(),
        health: local.health.clone(),
        events: local.events.clone(),
        auth_limiter: AuthObservationLimiter::default(),
        model_preparing: Arc::default(),
        shutdown: Arc::default(),
        transcription: local.transcription.clone(),
        developer_control: None,
        control: None,
        dictations: dictation::Dictations::new(None),
        streams: local.streams.clone(),
        stream_engine: local.stream_engine.clone(),
        meeting_events: meeting_events::Source::default(),
        remote: None,
        processing: local.processing.with_gate(processing_enabled),
        timestamps: local.timestamps,
    }
}

fn supervise(template: HttpContext, state: Arc<State>, stop: Arc<AtomicBool>, local_port: u16) {
    let mut active: Option<(SocketAddr, Arc<AtomicBool>, JoinHandle<()>)> = None;
    let mut refresh = Instant::now();
    let mut address = None;
    while !stop.load(Ordering::Acquire) {
        let config = CONFIG.read().unwrap_or_else(|e| e.into_inner()).clone();
        if !config.enabled {
            state.cancel();
            state.status("Off", None);
            if let Some(devices) = state
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_mut()
            {
                devices.clear_pair();
            }
            refresh = Instant::now();
            address = None;
        } else if Instant::now() >= refresh {
            address = network::detect()
                .ok()
                .filter(|_| config.port != local_port)
                .and_then(|ip| bind_address(ip, config.port).ok());
            refresh = Instant::now() + Duration::from_secs(5);
        }
        if active
            .as_ref()
            .is_some_and(|(bound, _, worker)| Some(*bound) != address || worker.is_finished())
        {
            let (_, shutdown, worker) = active.take().unwrap();
            shutdown.store(true, Ordering::Release);
            let _ = worker.join();
        }
        if config.enabled && enabled() && active.is_none() {
            let mut devices = state.devices.lock().unwrap_or_else(|e| e.into_inner());
            if devices.is_none() {
                *devices = crate::app_paths::support_dir()
                    .and_then(|path| Devices::load(path.join("remote-devices.json")))
                    .ok();
            }
            if devices.is_none() {
                state.status("Remote device store unavailable", None);
            } else if let Some(bound) = address {
                match TcpListener::bind(bound).and_then(|l| {
                    l.set_nonblocking(true)?;
                    Ok(l)
                }) {
                    Ok(listener) => {
                        let mut context = remote_context(&template);
                        context.remote = Some(state.clone());
                        let shutdown = context.shutdown.clone();
                        *state.shutdown.lock().unwrap_or_else(|e| e.into_inner()) =
                            Arc::downgrade(&shutdown);
                        let worker_state = state.clone();
                        let (ready, readiness) = mpsc::sync_channel(1);
                        let worker = thread::spawn(move || {
                            serve(listener, context, ready, Some(worker_state))
                        });
                        if readiness
                            .recv_timeout(Duration::from_secs(1))
                            .unwrap_or(false)
                        {
                            state.status("Listening", Some(bound));
                        } else {
                            state.status("Remote listener unavailable", None);
                        }
                        active = Some((bound, shutdown, worker));
                    }
                    Err(_) => state.status("Could not bind the Tailscale address and port", None),
                }
            } else {
                state.status("No Tailscale address", None);
            }
        }
        // Configuration changes, shutdown and Tailscale reconnects remain bounded.
        thread::sleep(Duration::from_millis(200));
    }
    state.cancel();
    if let Some((_, shutdown, worker)) = active {
        shutdown.store(true, Ordering::Release);
        let _ = worker.join();
    }
}

pub(super) fn bind_address(ip: IpAddr, port: u16) -> Result<SocketAddr> {
    if !matches!(ip, IpAddr::V4(_)) || !peer_allowed(ip) || port < 1024 {
        return Err(eyre!(
            "A Tailscale IPv4 address and nonzero port are required"
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

pub(super) fn peer_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            o[0] == 100 && (64..128).contains(&o[1])
        }
        IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn allowed(method: &str, path: &str) -> bool {
    match (method, path) {
        ("GET", "/health" | "/capabilities")
        | ("POST", "/transcriptions" | "/streams" | "/pair") => true,
        _ => streams::stream_path(path).is_some_and(|(_, action)| {
            matches!(
                (method, action),
                ("GET", "events") | ("POST", "audio" | "finish" | "cancel" | "heartbeat")
            )
        }),
    }
}

pub(super) fn request(
    request: HttpRequest,
    context: &HttpContext,
    state: Arc<State>,
    observation: &mut Option<Observation>,
) -> RequestAction {
    request_under_gate(request, context, state, observation, enabled())
}

fn request_under_gate(
    mut request: HttpRequest,
    context: &HttpContext,
    state: Arc<State>,
    observation: &mut Option<Observation>,
    enabled: bool,
) -> RequestAction {
    let respond = RequestAction::Respond;
    if !enabled {
        return respond(HttpResponse::code(503, "remote-disabled"));
    }
    let path = request.path.split('?').next().unwrap_or("/");
    if !allowed(&request.method, path) {
        return respond(HttpResponse::empty(404));
    }
    let mut devices = state.devices.lock().unwrap_or_else(|e| e.into_inner());
    let Some(devices) = devices.as_mut() else {
        return respond(HttpResponse::empty(503));
    };
    if devices.limited(Instant::now()) {
        return respond(HttpResponse::code(429, "auth-rate-limited"));
    }
    if path == "/pair" {
        return json_post_action(
            request,
            true,
            "pairing-unavailable",
            MAX_PAIR_BYTES,
            "request-too-large",
            |content_length, body_prefix| RequestAction::RemotePair {
                content_length,
                body_prefix,
                state: state.clone(),
            },
        );
    }
    let token = request
        .authorization
        .as_deref()
        .and_then(|a| a.strip_prefix("Bearer "))
        .unwrap_or("");
    let Some(device) = devices.authenticate(token) else {
        return respond(HttpResponse::empty(401));
    };
    *observation = Some(Observation {
        events: context.events.clone(),
        name: device.name,
        operation: if path.starts_with("/streams") {
            "stream"
        } else if path == "/transcriptions" {
            "transcription"
        } else {
            "probe"
        },
        started: Instant::now(),
        outcome: "connection-closed".into(),
    });
    // The remote caller cannot select or prepare another model. The standard
    // service owns inference, fallback, cancellation and dictation priority.
    if path == "/transcriptions" {
        let Some(selection) = crate::transcription::engine::TranscriptionEngine::shared()
            .and_then(|e| e.held_selection())
        else {
            return respond(HttpResponse::code(409, "model-not-loaded"));
        };
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in url::form_urlencoded::parse(
            request
                .path
                .split_once('?')
                .map(|(_, q)| q)
                .unwrap_or("")
                .as_bytes(),
        ) {
            if matches!(name.as_ref(), "model" | "language") {
                return respond(HttpResponse::code(400, "uses-loaded-model"));
            }
            query.append_pair(&name, &value);
        }
        query
            .append_pair("model", selection.model.as_str())
            .append_pair("language", &selection.language);
        request.path = format!("/transcriptions?{}", query.finish());
    }
    super::authenticated_request(request, context)
}

pub(super) fn pair(
    stream: &mut TcpStream,
    length: usize,
    mut body: Vec<u8>,
    state: &State,
    shutdown: &AtomicBool,
) -> HttpResponse {
    if let Err(error) = read_body_bounded(
        stream,
        length,
        &mut body,
        shutdown,
        MAX_PAIR_BYTES,
        Duration::from_secs(2),
    ) {
        return body_read_response(error);
    }
    if !enabled() {
        return HttpResponse::code(503, "remote-disabled");
    }
    let Ok(request) = serde_json::from_slice::<PairRequest>(&body) else {
        return HttpResponse::empty(400);
    };
    let mut devices = state.devices.lock().unwrap_or_else(|e| e.into_inner());
    let Some(devices) = devices.as_mut() else {
        return HttpResponse::empty(503);
    };
    if devices.limited(Instant::now()) {
        return HttpResponse::code(429, "auth-rate-limited");
    }
    match devices.pair(request, Instant::now()) {
        Ok((id, token)) => {
            HttpResponse::json(201, &serde_json::json!({"deviceId":id,"token":token}))
        }
        Err(_) => HttpResponse::code(401, "pairing-refused"),
    }
}

/// Metadata only: no URL queries, tokens, audio, response bodies, or text.
pub(super) struct Observation {
    events: EventLog,
    name: String,
    operation: &'static str,
    started: Instant,
    outcome: String,
}
impl Observation {
    pub(super) fn response(&mut self, response: &HttpResponse) {
        self.outcome = response.status.to_string();
    }
    pub(super) fn stream(&mut self) {
        self.outcome = "stream-response-ended".into();
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        let _ = self.events.emit(&VoiceEvent::RemoteRequest {
            timestamp_ms: now_ms(),
            device: self.name.clone(),
            operation: self.operation.into(),
            duration_ms: self.started.elapsed().as_millis() as u64,
            outcome: self.outcome.clone(),
        });
        tracing::info!(device = %self.name, operation = self.operation, duration_ms = self.started.elapsed().as_millis() as u64, outcome = %self.outcome, "remote dictation request");
    }
}
