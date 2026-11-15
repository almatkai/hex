# Dictate from a phone

Remote dictation is a Labs opt-in, off by default, audience Everyone, group
Dictation. The Android client is not built. Contract:
[remote-dictation.md](../remote-dictation.md); design/status:
[Android plan](../plans/android-remote-dictation.md).

```ts
Labs › Remote dictation                       // remote.enable
├─ Off (default) -> no tailnet socket or Tailscale probing
└─ On -> Remote dictation joins the sidebar
   ├─ no Tailscale IPv4 -> state explains why; retries every 5 s
   ├─ listening -> address and paired device list
   ├─ Pair device / hex remote pair             // remote.pair
   │  └─ five-minute one-use QR/payload -> POST /pair -> device token
   ├─ Remove… / hex remote revoke <id>          // remote.revoke
   │  └─ persist revoked token -> cancel remote work -> refuse future auth
   ├─ phone WAV -> existing transcription service -> returned text
   └─ phone stream -> existing stream owner -> dictation yields first
```

Settings are `remote_dictation.enabled`, `.port` (47821 by default), and
`.process_transcripts` (off). Each has a release `hex settings` key. The
listener is app-owned on Windows, Linux and macOS; service helpers do not
start it. It binds only a validated Tailscale IPv4, never the local API port
or a wildcard, and refuses peers outside the tailnet IP ranges. An exact
allowlist excludes app control, microphone capture, meetings and model
preparation. Only authenticated health/capabilities, whole WAV clips and
the existing stream routes are exposed, plus code-gated pairing.

Phone results stay out of History and Paste Last and are never pasted by
the PC. Optional processing reuses Global mode and its privacy choices;
the remote switch is separate from Process API transcripts. Whole clips
retain the service's existing long-clip/model-copy fallback; streams retain
its single shared admission, lease, bounds and dictation priority.

Sources: [remote listener](../../src/local_api/remote.rs),
[device store](../../src/local_api/remote/devices.rs),
[pane](../../src/rune_ui/remote.rs),
[settings](../../src/app_settings/remote.rs),
[Labs](../../src/labs.rs). Tests beside the listener and store cover
remote.enable/pair/revoke, peer/bind refusal, route isolation, limits and
rate limiting; local API tests cover the reused handlers. The
`remote-dictation` scene is a fixture, not native proof. Linux goldens are
pending; no real phone, firewall, QR scanner or speech engine was exercised
for this feature. New translations await native-speaker review.

Windows firewall provisioning is manual and scoped as the contract
explains; Hex never adds a broad exception or elevates. A sleeping PC stays
unreachable. HTTPS and remote History remain undecided. See the plan for
Android work and real-device acceptance checks.


Observed on Windows (2026-11-15): local build and strict Clippy passed;
remote/API/Labs/i18n filters passed (15/54/66/25 tests); settings had 136
passes and the three known Russian-locale failures. All three remote scenes
and Windows Labs were rendered and inspected in light and dark. The
renderer emitted LLVM legalization diagnostics while still producing the
inspected PNGs. These are fixture checks, not tailnet or native interaction
proof. The existing Rune pin is retained after a dev-head update required
an unavailable published dependency feature; see the plan's verification
record. No GitHub Actions were used.
