//! Remote dictation settings. Device-store writes stay off the window thread.
use super::*;
use crate::local_api::remote::{self as listener, Command, Snapshot};
use controls::{ButtonKind, button, note, page, section, setting_row, toggle_row};

#[derive(Default)]
pub(super) struct RemotePane {
    snapshot: Snapshot,
    result: Option<String>,
    pending: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    refresh_at: f64,
    snapshot_pending: Option<std::sync::mpsc::Receiver<Snapshot>>,
    confirm: Option<String>,
    payload: Option<(String, Instant)>,
    qr: Option<rune::Pixels>,
}

fn ask(ui: &mut Ui, command: Command) {
    if ui.is_fixture() || ui.remote.pending.is_some() {
        return;
    }
    let pairing = matches!(command, Command::Pair);
    let (send, recv) = std::sync::mpsc::sync_channel(1);
    ui.remote.pending = Some(recv);
    ui.remote.result = None;
    if pairing {
        ui.remote.payload = None;
    }
    std::thread::spawn(move || {
        let result = listener::command(command).map_err(|e| e.to_string());
        let _ = send.send(result);
    });
}

pub(super) fn poll(ui: &mut Ui, clock: f64) -> bool {
    if ui.pane != Pane::Remote || ui.labs_open {
        return false;
    }
    let mut changed = false;
    if let Some(result) = ui
        .remote
        .pending
        .as_ref()
        .and_then(|recv| recv.try_recv().ok())
    {
        ui.remote.pending = None;
        match result {
            Ok(value) if value.contains("\"hex-remote-pair\"") => {
                ui.remote.qr = qr(&value);
                ui.remote.payload = Some((value, Instant::now() + Duration::from_secs(300)));
            }
            Ok(value) | Err(value) => ui.remote.result = Some(value),
        }
        changed = true;
    }
    if ui
        .remote
        .payload
        .as_ref()
        .is_some_and(|(_, expires)| Instant::now() >= *expires)
    {
        ui.remote.payload = None;
        ui.remote.qr = None;
        changed = true;
    }
    if let Some(snapshot) = ui
        .remote
        .snapshot_pending
        .as_ref()
        .and_then(|recv| recv.try_recv().ok())
    {
        ui.remote.snapshot_pending = None;
        ui.remote.snapshot = snapshot;
        changed = true;
    }
    if !ui.is_fixture() && ui.remote.snapshot_pending.is_none() && clock >= ui.remote.refresh_at {
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        ui.remote.snapshot_pending = Some(recv);
        std::thread::spawn(move || {
            let _ = send.send(listener::snapshot());
        });
        ui.remote.refresh_at = clock + 1.0;
    }
    changed
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let snapshot = &ui.remote.snapshot;
    let address = snapshot.address.as_deref().unwrap_or("—");
    let rows = vec![
        setting_row(p, "status", tr("Status"), Some(tr(&snapshot.status)), el()),
        setting_row(p, "address", tr("Address"), Some(address), el()),
        setting_row(
            p,
            "pair",
            tr("Paired devices"),
            None,
            button(
                p,
                "pair-device",
                tr("Pair device"),
                ButtonKind::Primary,
                |ui, _| ask(ui, Command::Pair),
            )
            .disabled(ui.remote.pending.is_some() || snapshot.address.is_none()),
        ),
        toggle_row(
            p,
            "process",
            tr("Process transcripts from the phone"),
            None,
            ui.settings.remote_dictation.process_transcripts,
            |ui| {
                ui.update(|s| {
                    s.remote_dictation.process_transcripts = !s.remote_dictation.process_transcripts
                });
            },
        ),
    ];
    let mut content =
        page("remote-content").child(section(p, "listener", tr("Remote dictation"), rows));
    if cfg!(target_os = "windows") {
        content = content.child(note("firewall", tr("If the phone cannot connect, allow this port only on the Tailscale interface in Windows Firewall."), p.muted()));
    }
    if let Some((payload, _)) = &ui.remote.payload {
        let payload_copy = payload.clone();
        if let Some(qr) = &ui.remote.qr {
            content = content.child(
                el().key("pairing-qr")
                    .label(tr("Pairing QR code"))
                    .w(256.0)
                    .h(256.0)
                    .pixels(qr),
            );
        }
        content = content.child(section(
            p,
            "pairing",
            tr("Pairing code expires in five minutes"),
            vec![setting_row(
                p,
                "payload",
                tr("Pairing payload"),
                Some(payload),
                button(
                    p,
                    "copy-pairing",
                    tr("Copy"),
                    ButtonKind::Secondary,
                    move |_, tx| tx.copy(payload_copy.clone()),
                ),
            )],
        ));
    }
    if let Some(result) = &ui.remote.result {
        content = content.child(note("result", result, p.muted()));
    }
    let mut devices = Vec::new();
    for device in &snapshot.devices {
        let id = device.id.clone();
        let confirm = ui.remote.confirm.as_ref() == Some(&id);
        devices.push(setting_row(
            p,
            &device.id,
            &device.name,
            Some(&device.id),
            button(
                p,
                "revoke",
                if confirm {
                    tr("Confirm remove")
                } else {
                    tr("Remove…")
                },
                if confirm {
                    ButtonKind::Danger
                } else {
                    ButtonKind::Secondary
                },
                move |ui, _| {
                    if confirm {
                        ui.remote.confirm = None;
                        ask(ui, Command::Revoke { id: id.clone() });
                    } else {
                        ui.remote.confirm = Some(id.clone());
                    }
                },
            ),
        ));
    }
    if !devices.is_empty() {
        content = content.child(section(p, "devices", tr("Paired devices"), devices));
    }
    super::scroll_body(p.k, "remote-scroll", content)
}

pub(super) fn scene(ui: &mut Ui) {
    ui.settings.remote_dictation.enabled = true;
    ui.pane = Pane::Remote;
    ui.remote.snapshot = Snapshot {
        address: Some("http://100.64.114.14:47821".into()),
        status: "Listening".into(),
        devices: vec![
            serde_json::from_value(serde_json::json!({
                "id": "phone-1", "name": "Android phone", "last_use_ms": 0,
            }))
            .expect("remote device fixture"),
        ],
    };
}

pub(super) fn pairing_scene(ui: &mut Ui) {
    scene(ui);
    let payload = serde_json::json!({"type":"hex-remote-pair", "version":1,
        "url":"http://100.64.114.14:47821", "code":"fixture-only-not-a-pairing-grant", "expiresInSeconds":300}).to_string();
    ui.remote.qr = qr(&payload);
    ui.remote.payload = Some((payload, Instant::now() + Duration::from_secs(300)));
}

pub(super) fn unavailable_scene(ui: &mut Ui) {
    scene(ui);
    ui.remote.snapshot.address = None;
    ui.remote.snapshot.status = "No Tailscale address".into();
}

fn qr(payload: &str) -> Option<rune::Pixels> {
    let code = qrcode::QrCode::new(payload.as_bytes()).ok()?;
    // QR symbols need a fixed black/white raster and four-module quiet zone.
    let modules = code.width();
    let width = (modules + 8) * 4;
    let mut rgba = vec![255; width * width * 4];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] == qrcode::Color::Dark {
                for dy in 0..4 {
                    for dx in 0..4 {
                        let offset = (((y + 4) * 4 + dy) * width + (x + 4) * 4 + dx) * 4;
                        rgba[offset..offset + 3].fill(0);
                    }
                }
            }
        }
    }
    let pixels = rune::Pixels::new();
    pixels.set(width as u32, width as u32, rgba);
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_pane_is_visible_only_with_its_lab_and_fixtures_do_no_work() {
        let mut ui = Ui::fixture_base();
        assert!(!Pane::Remote.shown(&ui));
        scene(&mut ui);
        assert!(Pane::Remote.shown(&ui));
        ask(&mut ui, Command::Pair);
        assert!(ui.remote.pending.is_none());
        ui.settings.remote_dictation.enabled = false;
        assert!(!Pane::Remote.shown(&ui));
    }

    #[test]
    fn remote_pairing_scene_has_a_qr_and_a_copyable_payload() {
        let mut ui = Ui::fixture_base();
        pairing_scene(&mut ui);
        let (payload, _) = ui.remote.payload.as_ref().unwrap();
        let value: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(value["version"], 1);
        let (width, height) = ui.remote.qr.as_ref().unwrap().size();
        assert_eq!(width, height);
        assert!(width >= 100);
    }
}
