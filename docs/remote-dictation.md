# Remote dictation API, version 1

Hex's **Remote dictation** Lab is off by default and available to everyone
on desktop builds. Enable it in Labs or with:

```sh
hex settings set remote_dictation.enabled true
hex remote pair
hex remote devices
hex remote revoke <device-id>
```

The running desktop app owns this listener. `hex service` and embedded
helpers do not start it. The app must already have a speech model loaded;
the remote API never prepares, downloads, or selects a model.

## Transport and pairing

The listener binds the IPv4 reported by `tailscale ip -4`, validated against
`100.64.0.0/10`. The default TCP port is **47821**;
`remote_dictation.port` accepts 1024–65535. It refuses port zero, a wildcard,
loopback, other address ranges, and the local API's current port. The
Tailscale address is checked again every five seconds while enabled; an
address change stops the old listener before binding the new address.
Peers must be in `100.64.0.0/10` or `fd7a:115c:a1e0::/48`; this version
listens on IPv4 only. Forwarded headers never affect peer checks.

HTTP runs inside Tailscale's encrypted WireGuard tunnel. Do not proxy the
loopback API or expose this port through a public proxy. The Android client
will need a cleartext HTTP exception limited to its paired tailnet host.
Tailnet ACLs remain useful even with device authentication.

**Pair device** in the Remote dictation pane or `hex remote pair` opens a
five-minute pairing window and returns this JSON payload (also shown as a
QR in the window):

```json
{"type":"hex-remote-pair","version":1,"url":"http://100.64.1.2:47821","code":"<random-one-use-code>","expiresInSeconds":300}
```

Opening another window invalidates the previous code. Codes stay in memory
only and disappear on restart or disabling the Lab. Treat the code and QR
as secrets. Pairing is the only route that needs no bearer token:

```http
POST /pair
Content-Type: application/json
Content-Length: <bytes>

{"code":"<one-use-code>","name":"My Android phone"}
```

The name must contain 1–80 UTF-8 bytes, have non-whitespace content and no
control characters. At most 32 devices may pair. A successful **201** reply
is `{"deviceId":"…","token":"…"}`. Store the token securely on the phone:
Hex returns it only once. Invalid, expired, reused codes, invalid names, and
the device cap answer **401** `pairing-refused`. Failed persistence never
admits a device. A lost success response requires a new pairing code.

Each later request, including health/capabilities, sends
`Authorization: Bearer <device-token>`. Tokens contain secure random bytes;
Hex stores only SHA-256 digests, compared in constant time, in
`remote-devices.json` through `durable::write_private`. Unix files are 0600;
Windows inherits the user's Application Support ACL. The file contains
device names, IDs and last-use times, updated at most once per minute.
No token is accepted from `local-api.json`.

After ten failed credentials/pairing codes in a minute, allowed routes
answer **429** `auth-rate-limited` until the fixed minute window resets.
This bounded global bucket prevents rotating peers from bypassing the
limit; it can also temporarily block valid devices. Authentication errors
do not log credentials or arbitrary request paths.

Revocation persists before dropping access. It closes active remote
requests and cancels remote-created streams, including other phones' work;
the listener then restarts. Local API streams and dictations are not
cancelled. Turning the Lab off closes the listener and cancels remote work;
pairings remain saved for the next enable. Native inference already running
may finish internally after cancellation; its result is not returned.

## Routes and responses

Only these method/path pairs are served:

```ts
Remote listener
├─ POST /pair                         // one-use pairing code, bounded JSON
├─ GET /health                        // bearer token required
├─ GET /capabilities                  // bearer token required
├─ POST /transcriptions               // bearer token + WAV
└─ /streams                           // bearer token on every request
   ├─ POST /streams                   // create
   ├─ POST /streams/{id}/audio
   ├─ GET  /streams/{id}/events
   └─ POST /streams/{id}/{heartbeat|finish|cancel}
```

All other paths/methods answer **404**, including `/control`, `/dev/*`,
`/dictations`, `/meetings/*`, `/models`, and `/profile/*`. Unknown paths
answer 404 before authentication. Health returns the existing Hex/API
version fields. Capabilities uses the local API shape but reports
`control`, `developerControl`, `serviceCapture`, and `dictationPartials`
false. `outputProcessing` reflects the remote processing setting and the
installed desktop processor.

### Whole clips

```http
POST /transcriptions
Authorization: Bearer <device-token>
Content-Type: audio/wav
Content-Length: <bytes>

<WAV bytes>
```

The result is `{"transcript":"…","durationMs":1234}`. Audio and text stay
in memory; nothing is pasted, copied, added to History, or used for Paste
Last. The selected **loaded** model and language supply the request's
selection. Passing `model` or `language` answers **400** `uses-loaded-model`;
without a loaded engine the response is **409** `model-not-loaded`.

The existing `transcription/service` handles decoding, normalization,
admission, inference, fallback, and disconnect cancellation. Its behavior
is unchanged: short matching clips use the dictation engine; long clips or
the existing shared-engine wait fallback can use the service's own copy of
that selection. This implementation does not promise one model allocation
for arbitrary whole clips. Use streams for long recordings that should
yield between windows.

`?process=global` adds `processed` using the existing Global output
processor, only with `remote_dictation.process_transcripts` enabled
(default false). This switch is independent of Labs › Process API
transcripts. Without permission it answers **403** `processing-disabled`;
without a processor, **409** `processing-unavailable`; processing admission
is shared with the local API (**429** `queue-full`). Processing can use the
user's configured external providers or trusted script host APIs.
The existing timestamps opt-in and errors also apply when requesting
`?timestamps=segment|word`.

### Streams

Follow [the streams contract](streams.md), including the returned
`X-Hex-Stream-Token` owner credential on subroutes. Both the device bearer
token and stream owner token are required. The one stream admission is
shared with the loopback API. Streams use the loaded model only, keep a
10-second lease, cut pause-aware windows up to 25 seconds, and yield to
dictation. GigaAM windows use the existing offline path. Processing at
finish uses the remote processing switch.

### Bounds

The listener shares the existing HTTP parser and worker implementation:
four workers, sixteen queued connections, 16 KiB headers, 64 headers,
2 KiB paths, a two-second header deadline and bounded socket I/O. Pairing
bodies are at most 1024 bytes with a two-second upload deadline. Whole WAV
uploads are at most 64 MiB and use the existing 30-second upload deadline,
hostile-WAV checks, normalized-audio bound, and one shared audio admission.
Stream creation bodies are at most 1024 bytes; each audio push is at most
256 KiB with the existing five-second deadline and bounded backlog.
Oversized bodies are rejected before allocation/read. Existing local API
status/error codes apply (400/408/411/413/415/429/503/504).

## Windows Firewall and verification limits

Hex does not elevate or install firewall rules. Windows may show its normal
firewall prompt when the Lab first listens. A broad application exception
is unnecessary: in Windows Defender Firewall with Advanced Security,
create an inbound TCP rule for the selected port, program Hex, local
Tailscale address, remote `100.64.0.0/10`, and the Tailscale interface alias.
Select Private only if that interface actually uses the Private profile.
Do not open LAN/public-interface access or change the adapter profile to
make a rule match. The pane points here if a phone cannot connect. This
manual, scoped rule is the safe subset of the plan's automatic firewall
request; automatic private-interface provisioning remains unimplemented.

`local_api::remote` tests cover address/peer refusal, exact routes,
pairing expiry/replacement/reuse, hashed persistence, authentication,
revocation, rate limits, HTTP bounds, and the default-off Lab. The normal
local API/stream tests cover shared parsing and inference admission with
fake engines. These do not prove a phone-to-PC upload, real-model inference,
firewall acceptance, or QR scanning. Request observations include only
device name, operation, elapsed duration and outcome; stream observations
mark handler completion rather than recording transcript events. No audio,
text, tokens, pairing payloads or arbitrary URL queries are logged.
