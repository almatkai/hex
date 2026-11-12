# Android remote dictation over Tailscale

Status: plan only, not scheduled (2026-11-13). Nothing here is built.

## Goal

Dictate on an Android phone into any app, with the speech recognised by
Hex on the PC (GigaAM Multilingual on the GPU, Kazakh and Russian) over
Tailscale. The phone records and inserts text; the PC does all the
recognition and processing. No model runs on the phone.

## Decisions (from the owner)

- Android only. iPhone is out of scope.
- Speech runs on the PC only. With the PC off or unreachable, dictation on
  the phone is unavailable and says so; there is no on-device fallback.
- No custom keyboard. A small floating dot appears when a text field is
  focused and the keyboard slides up; it disappears with the keyboard.
- The phone and the PC talk only inside the owner's tailnet.

## How it works

```ts
Phone (Android app "Hex Remote")
├─ Accessibility service
│  ├─ sees: focused node is editable, not a password, IME window visible
│  ├─ shows: the dot (TYPE_ACCESSIBILITY_OVERLAY), just above the keyboard
│  └─ inserts: final text at the cursor of the focused node
├─ Recorder: AudioRecord, 16 kHz mono PCM16, VOICE_RECOGNITION source
└─ Client: HTTP over Tailscale (WireGuard) to the PC
      │
      ▼  http://<pc>.<tailnet>.ts.net:<port>   (or the PC's 100.x address)
PC (Hex, Labs › Remote dictation, off by default)
├─ Remote listener: bound to the Tailscale address only, fixed port
│  ├─ auth: per-device token from pairing (not the local API token)
│  └─ routes: /health, /capabilities, POST /transcriptions, /streams*
└─ Same engine as the shortcut: the loaded dictation model and language,
   dictation always first, optional output processing (process=global)
```

## PC side: Hex changes

Today's local API cannot be reached from the phone, and that is by design:
it binds `127.0.0.1` on a random port (`src/local_api.rs`,
`TcpListener::bind("127.0.0.1:0")`) and generates a new bearer token at every
launch, written only to the owner-only `local-api.json`. Exposing it as is
(for example with `tailscale serve` in front of it) would also expose
`/control`, which can drive the whole app. So the phone gets its own narrow
listener instead.

1. **Labs feature** `remote-dictation` (group Dictation, audience Everyone,
   off by default), settings `remote_dictation.enabled` and
   `remote_dictation.port` (default fixed, for example 47821), each with a
   `hex settings` key. Gate the listener's start on the feature, not only the
   UI.
2. **Remote listener** (`src/local_api/remote.rs`): a second `TcpListener`
   on the PC's Tailscale IPv4 (the local interface address in
   `100.64.0.0/10`, re-resolved when Tailscale reconnects), never
   `0.0.0.0`. It also refuses any peer outside `100.64.0.0/10` and
   `fd7a:115c:a1e0::/48`. It reuses the existing HTTP parsing, limits and
   handlers, but routes only:
   - `GET /health`, `GET /capabilities`
   - `POST /transcriptions` (whole WAV clip; the first version uses this)
   - `/streams` and its sub-routes (long dictations, phase 3)

   Everything else (`/control`, `/dev/*`, `/dictations`, `/meetings/*`,
   `/models` preparation, `/profile/*`) answers 404 on this listener.
3. **Pairing and device tokens**: Settings shows a QR code holding the URL
   and a one-time pairing code (valid for 5 minutes). The phone trades it
   (`POST /pair`) for a device token, which Hex keeps hashed in owner-only
   `remote-devices.json` with the device name and last use. Settings lists
   paired phones with Remove. Failed auth goes through the existing
   `AuthObservationLimiter` style limits.
4. **Windows Firewall**: a listener on a new port triggers the firewall
   prompt. Hex asks once, for the Tailscale interface's private profile only,
   and Settings explains how to allow it if it was declined.
5. **Settings pane** (sidebar item while the lab is on): status (listening on
   `<name>:<port>`, or why not: Tailscale not running or no Tailscale
   address), the pairing QR, paired phones, and "Process transcripts from the
   phone" (the existing `process=global` path: replacements, modes,
   transformations).
6. **Observations and History**: events record the request, the device name,
   the duration and the outcome, never the text. Whether phone dictations go
   to History is an open question (below).
7. **Docs and checks**: a feature map entry in `docs/features/`, the contract
   in `docs/remote-dictation.md`, unit tests for the route allowlist, peer
   filter, pairing expiry, and token revocation, and goldens for the new pane
   (regenerated on Linux).

## Phone side: the Android app

Kotlin, minSdk 29 (Android 10), sideloaded APK for personal use. The
Play Store's accessibility-service policy only matters if it is ever
published.

1. **Accessibility service** (the core): listens to window and focus events
   with `flagRetrieveInteractiveWindows`. The dot shows only while the
   focused node `isEditable` and not `isPassword`, and an
   `AccessibilityWindowInfo.TYPE_INPUT_METHOD` window is on screen. It is
   positioned just above that window's bounds, can be dragged, and its offset
   is remembered per app.
2. **Dot**: a `TYPE_ACCESSIBILITY_OVERLAY` window (no "draw over other apps"
   permission needed). Tap starts, tap again stops; hold to talk also works.
   It shows idle, recording (with level), sending, and error states, with
   haptics. Swipe it away to cancel the current recording. It turns grey when
   the PC doesn't answer `/health` (checked when it appears, at most once a
   minute).
3. **Recording**: `AudioRecord` at 16 kHz mono PCM16 into memory (32 KB/s,
   so a minute is about 1.9 MB, fine over Tailscale), then a WAV upload to
   `POST /transcriptions`. A foreground service of type `microphone` holds
   the recording.
4. **Inserting text**: splice the transcript into the focused node's text at
   its selection and apply it with `ACTION_SET_TEXT`. Where an app ignores
   that (some WebViews and custom editors), put the text on the clipboard and
   send `ACTION_PASTE`; failing that, leave it copied and say so.
5. **Setup screen**: scan the PC's QR to pair, check Tailscale is connected,
   grant the microphone, enable the accessibility service, and a test button
   that records and shows the transcript.

## Phases

Each phase ends with a check on the real phone and PC.

0. **Spike (half a day)**: confirm on the owner's phone that an
   accessibility service can (a) see the keyboard window and the focused
   field in their daily apps (Telegram, WhatsApp, Chrome), (b) start
   microphone capture while another app is in front (Android 11+ background
   microphone rules; the accessibility service and foreground-service
   exemptions must hold on this device), and (c) insert text with
   `ACTION_SET_TEXT` or paste. A failure here changes the design before any
   Hex work.
1. **Hex remote listener**: Labs toggle, Tailscale bind, route allowlist,
   pairing, device tokens, firewall. Check: `curl` from another tailnet device
   transcribes a WAV, and `/control` answers 404.
2. **Android v1**: the dot, recording, upload to `/transcriptions`, insertion
   with fallbacks, pairing screen. Check: dictate Kazakh and Russian into
   Telegram and Chrome.
3. **Long dictations**: switch to `/streams` (16 kHz `s16le` pushed while
   recording, pause-cut windows up to 25 s, a 10 s lease kept alive by
   heartbeats) so long dictations start transcribing before they end. GigaAM
   doesn't stream, so its windows run offline as each pause closes them.
4. **Polish**: settings pane goldens, docs, per-app dot offset, a quick
   language switch (kk/ru) on long-press if the selected model needs a hint.

## Who builds what

Following the routing the owner set: the Hex listener and pairing (security
boundary) and the Android accessibility service and insertion → Codex Sol 6.1.
The phase 0 spike, Gradle project and setup screens → Antigravity. Every
phase's review → Opus. All checks run locally, never on GitHub Actions.

## Open questions

- Should phone dictations appear in Hex History (kind "Remote"), or stay
  out of it?
- Is HTTP inside the tailnet enough (WireGuard already encrypts), or should
  Hex serve HTTPS through `tailscale serve` with a `ts.net` certificate? HTTP
  needs a cleartext exception for the tailnet host in the Android app's
  network security config.
- Should the PC be kept awake while remote dictation is on? Today a sleeping
  PC just makes the dot go grey.

## Risks

- Android vendors (Xiaomi, Samsung and others) kill or restrict background
  services and accessibility services. The app needs the battery-optimisation
  exemption and may still need per-vendor steps.
- Some apps don't expose their text fields to accessibility or ignore
  `ACTION_SET_TEXT`. The clipboard fallback covers most of them.
- This opens a network door into Hex, which is otherwise local. The narrow
  route list, Tailscale-only bind, pairing and revocable tokens, and the
  Labs default-off keep that door small.
