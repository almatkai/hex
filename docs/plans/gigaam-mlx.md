# GigaAM Multilingual on MLX (macOS) — design, not implemented

Goal: on Apple Silicon Macs, run ai-sage's GigaAM-Multilingual (600M
Conformer, char-wise CTC, MIT) through MLX with ai-babai's converted
weights, instead of transcribe.cpp's Metal GGUF build of the same model,
without shipping Python. Windows and Linux keep the GGUF
(`vladkens/gigaam-multilingual-large-ctc-gguf`, Q8_0), which a separate task
adds to the catalog and which also runs on macOS through the existing Metal
path.

Designed on 2026-11-12 on a Windows machine. **Nothing here has been
compiled or run.** The skeleton code is a careful port of the reference, not
working software; every `ASSUMPTION` comment in it needs a Mac to settle.

## Recommendation

1. Runtime: MLX's **C++ API behind a small C ABI**, the transcribe.cpp shape
   (`native/gigaam_mlx/`), built by `build.rs` through CMake, and called
   from Rust. Not mlx-swift (needs xcodebuild/SwiftPM inside Cargo), not
   mlx-c (hundreds of hand-freed handles in Rust).
2. A **separate model id**, `gigaam_multilingual_mlx`, available only on
   aarch64 macOS, behind a **Labs** toggle that is off by default (AGENTS.md:
   user-requested features land in Labs). Same languages and behaviour as
   `gigaam_multilingual` (GGUF); the two can be benchmarked side by side.
3. Default artifact: `ai-babai/gigaam-multilingual-mlx-int8-g64` at commit
   `03a2921c…` (tag `v0.1.0`), three files, 699 MB, all SHA-256-pinned in
   Hex; FP16 is a second pin for parity work only.
4. **Measure before porting.** MLX is not known to beat transcribe.cpp's
   Metal GGUF for this model. Phase 0 compares the Python package with the
   GGUF on your own clips in an hour; if MLX isn't clearly faster or more
   accurate, stop and use the GGUF on macOS too.
5. Effort if it goes ahead: about 2–3 weeks of focused work on a Mac, plus
   Dax's review for the CI change that installs Xcode 26's Metal toolchain.

## Sources (pinned)

```ts
Reference implementation
├── PyPI gigaam-multilingual-mlx 0.2.0 sdist        // read in full
│   ├── model.py          // the MLX forward pass this port mirrors
│   ├── service.py        // 20 s / 2 s chunking, word times, keep rule
│   ├── quantization.py   // weight_only_affine, all_linear, group 64
│   ├── artifacts.py      // variants, REQUIRED_FILES, manifest checks
│   └── parity.py         // its own PyTorch parity thresholds
├── HF ai-babai/gigaam-multilingual-mlx           tag v0.1.0 = ca69664…  (FP16)
├── HF ai-babai/gigaam-multilingual-mlx-int8-g64  tag v0.1.0 = 03a2921c… (default)
│   └── model.safetensors headers read with HTTP ranges (tensor names, dtypes, shapes)
└── Upstream
    ├── HF ai-sage/GigaAM-Multilingual  revision 3905cd51…  config.json
    └── github.com/salute-developers/GigaAM  encoder.py, preprocess.py, model.py, decoding.py
MLX
├── ml-explore/mlx v0.32.4 = 36969c09…  ops.h, fast.h, fft.h, io.h, memory.h,
│                                       backend/metal/metal.h, metal/device.cpp
├── ml-explore/mlx-swift 0.33.0, mlx-c v0.8.0 (README, Package.swift, CMakeLists)
└── PyPI mlx-metal 0.32.0 wheel: 40.8 MB (macOS 14/15), 56.5 MB (macOS 26)
```

## The Model, Exactly

Two corrections to the brief: attention is **rotary**, not relative-position
(`self_attention_model: "rotary"`), and the rotation is applied to the layer
input before the q/k projections. The conv kernel is 5, not Conformer's
usual 31.

```ts
audio: 16 kHz mono f32 in [-1, 1], N samples (one chunk ≤ 20 s)
├── LogMelFrontend                       // float32 throughout
│   ├── N < 320 → pad to 320, but length 0: decodes to "" (no GPU work in Hex)
│   ├── frames: as_strided [1, F, 320], hop 160, F = 1 + (N − 320) // 160
│   │   (center = False: no reflect padding, no dither, no pre-emphasis)
│   ├── × window                         // tensor preprocessor.window [320] f32
│   │                                    // (torchaudio Hann, periodic; stored, not recomputed)
│   ├── rfft n_fft = 320 → [1, F, 161] complex; |·|² (power 2)
│   ├── @ mel_filter                     // tensor preprocessor.mel_filter [161, 64] f32
│   │                                    // (torchaudio MelScale: htk, 0–8 kHz, no norm)
│   └── ln(clip(x, 1e-9, 1e9))           // no mean/variance normalisation
│       → astype(float16) [1, F, 64]
├── StridingSubsampling (×4)             // NLC conv1d, MLX weight layout [out, k, in]
│   ├── conv1 64→1024, k 5, stride 2, pad 2, + bias, ReLU
│   ├── conv2 1024→1024, k 5, stride 2, pad 2, + bias, ReLU
│   └── length L → floor((L + 4 − 5) / 2 + 1), twice; T ≈ F / 4 (40 ms frames)
├── Rotary tables (once per chunk)
│   ├── head_dim 64, base 5000 (upstream pos_emb_max_len), inv_freq = 1 / 5000^(2i/64)
│   ├── emb = concat(t·inv_freq, t·inv_freq) [T, 64] → astype(float16)
│   └── cos(emb), sin(emb)               // reference casts to fp16 BEFORE cos/sin;
│                                        // upstream PyTorch does cos/sin in f32 (see Risks)
├── 24 × ConformerLayer (d 1024, 16 heads, d_ff 4096), batch 1, no masks
│   ├── r = x + 0.5 · FF1(LN(x))         // FF = Linear 1024→4096, SiLU, Linear 4096→1024
│   ├── r = r + MHSA(LN(r))
│   │   ├── h = LN(r) as [1, T, 16, 64]; h_rot = h·cos + rotate_half(h)·sin
│   │   ├── q = Wq(h_rot), k = Wk(h_rot), v = Wv(h)   // v is NOT rotated
│   │   ├── softmax(q kᵀ · 64^−0.5) v → Wout
│   │   └── (upstream batch>1 masks with −10000; Hex runs batch 1)
│   ├── r = r + Conv(LN(r))
│   │   ├── pointwise conv1 1024→2048 (k 1), GLU on channels
│   │   ├── depthwise conv k 5, pad 2, groups 1024, + bias
│   │   ├── LayerNorm (named batch_norm in the checkpoint; it is a LayerNorm)
│   │   └── SiLU, pointwise conv2 1024→1024 (k 1)
│   ├── r = r + 0.5 · FF2(LN(r))
│   └── LN_out(r)                        // every LayerNorm: eps 1e-5, affine
├── CTC head: conv1d 1024→71 (k 1) + bias, log_softmax over 71
└── Greedy decode (host)
    ├── argmax per frame, frames [0, T); blank = 70 (= len(vocabulary))
    ├── drop blanks and repeats of the previous frame's label
    ├── vocabulary (70): " ", "'", a–z, а–я, ё, і, ғ, қ, ң, ү, ұ, һ, ә, ө
    │   → lower case, no punctuation, no digits
    └── words split on " "; times frame·shift .. (last+1)·shift,
        shift = chunk_samples / 16000 / T  (CTC emission times, not alignment)
```

Long audio: upstream `transcribe()` refuses more than 25 s
(`LONGFORM_THRESHOLD`). The MLX package cuts fixed 20 s windows with 2 s
overlap (`step 18 s`) and keeps a word when its midpoint lies in
`[start + 1 s, end − 1 s)` (no trim at the outer edges), then makes word
times monotonic and joins words with spaces; if no word survives it joins
the chunks' raw texts. The C++ port reproduces this exactly so its text can
be compared byte for byte with `gigaam-stt`.

Numerics: compute dtype is the config's `float16`; the frontend and its two
tensors stay float32. Python's scalars are weakly typed (`0.5 * x` stays
fp16); the C++ port builds every scalar in the graph's dtype for the same
reason (`scalar()` in the skeleton). Softmax is MLX's default (non-precise).

## Artifacts And Pins

```ts
models/gigaam-multilingual-mlx-int8-g64/        // Hex's models folder, one directory
├── config.json        1,149 B    72c544eec78996dc57ed88f8308d735647644e981ec81f46ce945ee1b027a174
├── manifest.json      2,232 B    fdae07e2f5bc8e5a30a5a76edf6d1acb014c609dbeb3df1fa2d7fa67b91e01f4
└── model.safetensors  698,960,933 B  efd4103be5875ba556e6645ce34dd7def2339c7377209c86b7dd5787fb443729
    // repository ai-babai/gigaam-multilingual-mlx-int8-g64, revision 03a2921c291e908dcfb29ca4a9a66b31bd304e42
    // 1,208 tensors; 192 quantized Linear (24 layers × q, k, v, out, ff1×2, ff2×2)
    // quantized Linear: weight U32 [out, in·8/32], scales F16 [out, in/64],
    //                   biases F16 [out, in/64], bias F16 [out]
    // never quantized: every Conv1d (subsampling, pointwise, depthwise, head), LayerNorms,
    //                  preprocessor.window, preprocessor.mel_filter (F32)
models/gigaam-multilingual-mlx/                  // FP16, parity reference only
├── config.json        1,149 B    72c544ee… (same bytes)
├── manifest.json      1,725 B    c3bb30af00ebf93119d94e0bdf189beb195ae41e60240310514bc933bc63bc49
└── model.safetensors  1,170,775,344 B  d6d7bcef5e1b77700c84d669635a88021724fd3b1ae0c408890bccdf3c8d2269
    // repository ai-babai/gigaam-multilingual-mlx, revision ca6966445496c7ab8c73b0e59cfd6ff68f188c75
    // 824 tensors, 585,330,119 parameters
```

Both manifests record their upstream source (`ai-sage/GigaAM-Multilingual`
revision `3905cd51…`, `pytorch_model.bin` sha256 `c3fabefb…`). Hex pins by
commit, never by the `v0.1.0` tag, and trusts its own compiled pins, not
the manifest's hashes. The INT6 and INT4 variants exist (573 and 447 MB);
they are left out until someone measures them on dictation.

## Runtime Choice

```ts
(a) mlx-swift (0.33.0)               fits the Swift-bridge habit, nothing else
├── + highest-level API (MLXNN modules, QuantizedLinear, loadArrays)
├── − Hex compiles each Swift bridge with one `swiftc -emit-object`; MLX
│     needs SwiftPM package resolution plus `xcodebuild`, because "SwiftPM
│     (command line) cannot build the Metal shaders" (its README)
├── − a SwiftPM static library inside a Cargo link: module maps, C++ interop,
│     Swift runtime rpaths, network fetch during `cargo build`
├── − three layers (Swift → mlx-c → C++) between Hex and the kernels
└── − metallib must ride in the SwiftPM bundle `mlx-swift_Cmlx.bundle`
(b) mlx-c (v0.8.0) from Rust          no C++ of our own
├── + the forward pass in Rust
├── − every op returns a handle Rust must free; ~40 ops × 24 layers of
│     unsafe FFI with error checks, or a third-party wrapper (mlx-rs)
│     that trails MLX releases
└── = same CMake build and metallib as (c), plus mlx-c on top
(c) MLX C++ (v0.32.4) + C ABI         RECOMMENDED
├── + the API Python binds directly: same ops, same quantized kernels,
│     so parity with gigaam-stt is the easiest to reach
├── + the transcribe.cpp shape Hex already lives with: native core,
│     C header, thin unsafe Rust wrapper, built by build.rs
├── + ~700 lines of C++, mostly a transliteration of model.py and service.py
├── + no Swift, SwiftPM, or xcodebuild; CMake + Xcode command-line tools
├── − C++ API churn across MLX releases (quantized_matmul's signature
│     changed in 0.2x); pinned, bumped deliberately
└── − Hex gains a C++ source tree to review
```

Build facts that hold for (a), (b), and (c) alike:

- **Metal compiler.** MLX's CMake runs `xcrun metal` to build
  `mlx.metallib`. Xcode 26 ships the Metal toolchain as a separate
  download (`xcodebuild -downloadComponent MetalToolchain`; confirm on the
  first build). ggml doesn't need it today because Hex embeds ggml's Metal
  source and compiles it at run time. Release and Check macOS runners need
  that step, a `.github/` change that needs Dax.
- **metallib location.** MLX v0.32.4 looks for `mlx.metallib` beside the
  binary, then `Resources/mlx.metallib` beside the binary (that would be
  `Contents/MacOS/Resources`, which isn't a bundle convention), then SwiftPM
  bundles, then a compile-time path. Hex calls
  `mlx::core::metal::set_metallib_path` (in `backend/metal/metal.h`;
  "must be called before any MLX operation") with
  `Hex.app/Contents/Resources/mlx.metallib`. Resources are sealed by the
  bundle signature; there is no nested code to sign.
- **JIT option.** `MLX_METAL_JIT=ON` compiles kernels from source at first
  use, which shrinks the bundle and costs first-dictation latency (prewarm
  hides it). Hardened runtime already allows this: ggml does the same in
  Hex today. Whether JIT still needs a small metallib is unconfirmed.
- **Size.** The `mlx-metal` 0.32.0 wheel (libmlx plus mlx.metallib) is
  40.8 MB compressed for macOS 14/15 and 56.5 MB for macOS 26. Expect the DMG
  to grow by roughly that. Measure it on the first build.
- **Strict Metal.** At load the library requires `metal::is_available()`,
  sets the default device to the GPU, and never selects the CPU. MLX raises
  instead of silently falling back, so a missing kernel fails the load and
  the saved choice stays as it is (the existing switch invariant).
- **Shutdown.** MLX has static destructors (device, allocator), like ggml.
  `desktop/process_exit` already `_exit`s after joining workers, which
  covers them.
- **Apple Silicon only.** MLX has no Intel Metal backend. Gate everything on
  `all(target_os = "macos", target_arch = "aarch64")`; Hex ships arm64
  DMGs only.

## C ABI

`native/gigaam_mlx/gigaam_mlx.h` (complete, documented). In short:

```ts
gigaam_mlx_load(config, options) → session          // options.model_dir, metallib_path,
│                                                   // weights_sha256 (NULL: Hex's receipt
│                                                   // already vouched), cache limit, prewarm
├── config: dims, rotary base, dtype, quant bits/group, vocabulary
│   └── parsed and validated in Rust (serde) from config.json + manifest.json;
│       C++ cross-checks every tensor's name, dtype, shape and refuses leftovers
├── gigaam_mlx_transcribe(session, f32*, n, options) → result
│   ├── options: chunk/overlap (0 = 20 s / 2 s), should_cancel(user) callback,
│   │   release_cache (clear MLX's free-buffer cache afterwards)
│   ├── cancel points: before each chunk and after layers 8 and 16
│   └── result: UTF-8 text; words {text, start_ms, end_ms}; tokens {id, frame, time_ms}
├── gigaam_mlx_log_probs(session, f32*, n, out, cap, &frames)   // parity hook, one pass
├── gigaam_mlx_memory(&active, &peak, &cache), gigaam_mlx_reset_peak_memory()
├── gigaam_mlx_version(), gigaam_mlx_metal_available()
└── *_free for session, result, and every error string
```

Statuses: `OK`, `INVALID`, `ARTIFACT` (missing file, bad hash, wrong
tensor), `NO_METAL`, `CANCELED`, `RUNTIME` (anything MLX threw, with its
message). Every call is serialized by one process-wide mutex in the library:
Hex runs inference on its one engine thread, but the local API's service can
hold a second transcriber.

Files in this branch:

- `native/gigaam_mlx/gigaam_mlx.h`: the ABI.
- `native/gigaam_mlx/gigaam_mlx.cpp`: the whole forward pass, decoding,
  chunking, and loader against MLX v0.32.4's headers. Untested.
- `native/gigaam_mlx/CMakeLists.txt`: fetches MLX by commit and builds a
  static `gigaam_mlx` plus MLX and `mlx.metallib`.
- `src/gigaam_mlx.rs`: the safe Rust wrapper (config/manifest parsing,
  load, transcribe into `Transcript`, log-probs, memory). **Not in the module
  tree**, so the branch builds exactly as `dev` does.
- `scripts/gigaam-mlx-reference.py`: developer-only reference dump from the
  Python package (Hex never ships or runs Python).

## Integration

```ts
Model id: gigaam_multilingual_mlx (new TranscriptionModelId variant, every OS)
├── why separate, not "same id, MLX on macOS"
│   ├── both runtimes side by side on one Mac: the only fair A/B
│   ├── the Labs gate fits ParakeetUltra's pattern (is_labs_gated, available())
│   ├── no runtime switch threaded through every `model.runtime` match
│   └── a failed MLX load leaves the GGUF choice and download untouched
├── graduation: if MLX wins, aarch64 macOS's gigaam_multilingual takes
│   ModelRuntime::Mlx, a settings migration rewrites the saved id once,
│   and the lab is removed; if it loses, drop the id and the lab
└── off macOS the id decodes but is never available (like a saved
    import that can't run): the listener runs the default and the
    saved choice is kept, never rewritten
Catalog (transcription/models, transcription/artifacts)
├── ModelRuntime::Mlx(&'static MlxArtifact)          // new variant
├── MlxArtifact { directory, repository, revision, files: &[MlxFile],
│                 architecture, variant }; MlxFile { name, bytes, sha256 }
├── ModelDefinition: name "GigaAM Multilingual (MLX)", streams false,
│   languages and flags exactly as the GGUF entry (ru, kk, ky, uz; no hint;
│   detection true like Granite's CTC head), vocabulary_use None
├── available(): labs::gigaam_mlx() && GigaAmMlx::metal_available()
├── size_label / download_bytes: sum of files (699 MB)
├── model_path → models_dir()/<directory>; is_installed: every file at its size
├── download: each file through net::download into <directory>.partial/,
│   size + SHA-256 checked against Hex's pins, then the directory renamed
│   into place under the existing .download.lock; one receipt per file
│   (same VerificationReceipt shape), so later loads skip hashing
├── verify_installed / remove_installed: directory-wide, same lock
└── later, not v1: found_models could adopt a Hugging Face cache snapshot
    left by `gigaam-stt` (models--ai-babai--…/snapshots/<commit>/), whose
    blob names are the SHA-256 pins; read-only, as every found model
Labs (labs::Feature)
├── GigaAmMlx: key labs.gigaam_mlx, group Models, audience Everyone,
│   on_this_os = cfg!(all(target_os = "macos", target_arch = "aarch64"))
├── off_refusal while it is the dictation model (as ParakeetUltra)
├── gate where work starts: Transcriber::load_selection refuses the Mlx
│   runtime while the lab is off; the download worker too
└── hex settings key, docs/features/labs.md, the model feature map
Runtime (transcription.rs, engine, meetings, service)
├── Transcriber::Mlx(Box<GigaAmMlx>)   // beside Gguf and AppleSpeech, macOS only
│   ├── load_selection: ModelRuntime::Mlx → GigaAmMlx::load(dir, None)
│   ├── transcribe*: pad_for_parakeet (1.5 s minimum, as every model),
│   │   no trailing silence; returns text, one segment, CTC word times
│   ├── timestamps(): TimestampLevel::Word
│   ├── prewarm(): no-op (loaded prewarmed; Unload when idle reloads)
│   └── stream_options(): None → dictation transcribes the whole clip after
│       release; Labs › Live text shows nothing for this model
├── engine::held and meeting::transcript::engine_model accept Mlx as well as
│   Gguf, or meetings and host clips would load a second copy (invariant)
├── cancellation: Transcriber has no cancel today; pass a never-set flag in
│   v1, wire the engine's shutdown/meeting fence later
├── capture words (Annotate): transcribe_with_words maps to the word times
└── benchmark: `hex benchmark-transcription --backend mlx --model <dir>`
Build
├── Cargo.toml: cmake = "0.1" in [build-dependencies] (plain table: target
│   tables for build-dependencies match the host, not the target)
├── build.rs, aarch64 macOS only: compile_gigaam_mlx()
│   ├── cmake::Config::new("native/gigaam_mlx")
│   │   .define("CMAKE_OSX_DEPLOYMENT_TARGET", "15.0").build()
│   ├── link static gigaam_mlx and mlx, c++, frameworks Metal, Foundation,
│   │   QuartzCore, Accelerate
│   └── copy mlx.metallib to target/<profile>/ and set
│       cargo:rustc-env=HEX_MLX_METALLIB for `cargo run`
├── a cargo feature `mlx`, off by default until Phase 2 passes, so
│   scripts/check and CI don't compile MLX (minutes, cold) for everyone
├── scripts/build-app.sh: copy mlx.metallib to Contents/Resources;
│   validate-app.sh: require it when the feature is on
├── THIRD_PARTY_NOTICES.md: MLX (MIT), gigaam-multilingual-mlx (MIT),
│   GigaAM (MIT); model_details: licence credit and links
└── CI (Dax): Metal toolchain download on macOS runners
platform::DIFFERENCES: no entry. It records differences in
`Capabilities` (panes and rows); this adds none. The picker lists the model
from the catalog and Labs' on_this_os, which is where per-OS lab
availability is already reviewed. If a reviewer prefers a capability, add
`mlx_models` with Difference::Impossible("MLX runs only on Apple Silicon's
Metal").
```

Skeleton code for the catalog, not yet applied (it would break exhaustive
matches until every site is updated):

```rust
pub enum ModelRuntime {
    Gguf(&'static GgufArtifact),
    /// A multi-file MLX artifact; aarch64 macOS only.
    Mlx(&'static MlxArtifact),
    AppleSpeech,
}

pub struct MlxFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub struct MlxArtifact {
    pub directory: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub files: &'static [MlxFile],
    pub architecture: &'static str,
    pub variant: &'static str,
}

impl MlxArtifact {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }
    fn download_url(&self, file: &MlxFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repository, self.revision, file.name
        )
    }
}

// ai-babai's MLX port (MIT) of ai-sage's GigaAM-Multilingual (MIT),
// INT8 group-64 weight-only quantization of every Linear.
pub(super) const GIGAAM_MULTILINGUAL_MLX_INT8_ARTIFACT: MlxArtifact = MlxArtifact {
    directory: "gigaam-multilingual-mlx-int8-g64",
    repository: "ai-babai/gigaam-multilingual-mlx-int8-g64",
    revision: "03a2921c291e908dcfb29ca4a9a66b31bd304e42",
    files: &[
        MlxFile {
            name: "config.json",
            bytes: 1_149,
            sha256: "72c544eec78996dc57ed88f8308d735647644e981ec81f46ce945ee1b027a174",
        },
        MlxFile {
            name: "manifest.json",
            bytes: 2_232,
            sha256: "fdae07e2f5bc8e5a30a5a76edf6d1acb014c609dbeb3df1fa2d7fa67b91e01f4",
        },
        MlxFile {
            name: "model.safetensors",
            bytes: 698_960_933,
            sha256: "efd4103be5875ba556e6645ce34dd7def2339c7377209c86b7dd5787fb443729",
        },
    ],
    architecture: "gigaam_multilingual_ctc_conv1d_rotary_conformer",
    variant: "int8-g64",
};
```

## Verification (on a Mac)

Use your own Kazakh and Russian dictation: 20–40 WAVs, 16 kHz mono 16-bit,
1–20 s, plus three long ones (45 s, 2 min, 5 min) to exercise chunking,
with hand-written references. Add a FLEURS kk_kz and ru_ru subset for a
public number (`benchmarks/multilingual-v1/manifests` in the sdist lists
them). Keep the files local; the transcription-benchmark manifest format is
in `docs/research/transcription-benchmark.md`.

```ts
Phase 0: is MLX worth porting? (no Hex code; ~1 hour)
├── uv run --python 3.13 --with gigaam-multilingual-mlx==0.2.0 \
│     python scripts/gigaam-mlx-reference.py --variant int8 --out ref-int8 clips/*.wav
├── same with --variant fp16 --out ref-fp16
├── Hex GGUF (the other task's catalog entry, Metal):
│     /usr/bin/time -l target/release/hex benchmark-transcription manifest.json \
│       --model <gigaam-multilingual Q8_0 .gguf> > gguf.jsonl
├── compare per clip: text, WER/CER vs your references, median and p95 ms,
│   peak memory (Python: metal_peak_bytes; Hex: time -l maximum resident)
└── go only if MLX int8 is clearly faster (say ≥ 1.5× median on 5–15 s
    clips) or clearly more accurate (CER lower beyond run-to-run noise);
    else stop: "GGUF on macOS too" is the honest outcome
Phase 1: parity of the native port with Python (exact)
├── a test binary, or `hex dev gigaam-mlx parity <dir> <ref-dir> <wavs>`
│   (debug build), calling gigaam_mlx_log_probs and gigaam_mlx_transcribe
├── FP16 artifact, single pass, vs ref-fp16/<stem>.logprobs.npy:
│   cosine ≥ 0.9999, normalised MAE ≤ 1e-3 (parity.py's own thresholds),
│   argmax token sequence identical
├── int8-g64 the same against ref-int8 (same MLX kernels: expect identity
│   or 1-ulp noise; any token difference is a bug)
├── chunked text and word times identical to ref-*/<stem>.json "chunked"
│   on the 45 s / 2 min / 5 min clips (keep rule, monotonic times)
├── edge cases: 0 samples, 319, 320, 321, exactly 20 s, 38 s (two chunks
│   meeting at the overlap), silence, clipping at ±1
└── strict load: a truncated safetensors, one renamed tensor, a wrong
    config.json value → ARTIFACT or INVALID, never a crash
Phase 2: in Hex, against the GGUF of the same model
├── hex benchmark-transcription manifest.json --backend mlx \
│     --model ~/Library/Application\ Support/hex/models/gigaam-multilingual-mlx-int8-g64
├── report beside gguf.jsonl: load (cold after install, warm), first clip,
│   median / p95 / max clip ms, WER/CER, peak resident memory
├── idle memory after a dictation with release_cache (expect ≈ weights,
│   0.7 GB) and with Labs › Unload model when idle
├── switching GGUF ↔ MLX in the picker: the old model is released, no
│   second copy, a failed MLX load keeps the old selection
├── meetings and a host clip on the engine's MLX model, no second load
└── signed Hex.app from scripts/build-app.sh: metallib found, Gatekeeper
    and codesign --verify --deep --strict pass, DMG size delta recorded
Decision: ship (Labs) only if Phase 1 is exact and Phase 2 keeps the
Phase 0 advantage. Record the numbers in docs/features (the model map).
```

The ai-babai report gives one data point (Apple M4 Pro, 48 GB): a 5-minute
Russian WAV in 2.04 s with int8-g64 (about 150× real time), 0.877 GB peak
process memory, 5.07% macro WER over ru/kk/ky/uz FLEURS. Hex has no Metal
GGUF number for this model yet, so nobody knows which is faster. For a
5–15 s dictation clip both will most likely land in tens of milliseconds,
where model load, padding, and paste dominate what you feel.

## Risks

- **No proven win.** The same weights at 8 bits run on the same GPU in
  both runtimes. Expect similar accuracy (INT8 g64 affine against Q8_0
  blocks of 32) and similar speed. MLX may be somewhat faster on long
  meeting audio and no better on short dictation. Phase 0 exists to find
  out cheaply.
- **The skeleton is unverified.** It was written against v0.32.4 headers
  without a compiler. Expect compile errors (designated initializers in a
  vector, the `array(T, Dtype)` constructor, `mx::version`,
  `get_cache_memory`) and a few days of parity debugging.
- **fp16 rotary angles.** The reference casts angles to fp16 before cos/sin.
  At 500 frames (20 s), fp16's step there is 0.25 rad, so the
  highest-frequency pair is perturbed. Parity needs the same cast. Its WER
  (5.07%) includes the effect. Computing the tables in f32 could help or do
  nothing; measure it as a separate change after parity.
- **Lower case, no punctuation.** The vocabulary has no capitals,
  punctuation, or digits. That's true of both runtimes, but dictation output
  looks raw without Output processing (modes, OpenCode rewrite,
  transformations).
- **Build cost.** MLX is a large C++ build (minutes, cold). Xcode 26's
  separate Metal toolchain is needed on every macOS CI runner (a `.github/`
  change, Dax). The bundle grows by tens of MB. The cargo feature keeps
  this off everyone's machine until it earns its place.
- **Two Metal runtimes in one process.** ggml and MLX each keep their own
  device state and caches. Only one model is held at a time, but a switch
  briefly holds both, as today.
- **MLX C++ API churn.** Pinned, but every bump means re-reading
  `ops.h`/`fast.h` signatures and running Phase 1 again.
- **Thread model.** The library's global mutex serializes everything. A
  long meeting chunk and a dictation can't overlap, which matches the engine's
  ordering already; the engine's yield rules still apply above it.
- **No streaming.** CTC could stream cheaply later (re-run the encoder over
  the growing buffer every ~1 s, or a chunked encoder), but v1 transcribes
  after release only, so Live text is blank for this model.
- **Upstream drift.** ai-babai could re-tag `v0.1.0`; Hex pins the commit,
  and a changed file fails its SHA-256 instead of loading.

## Effort

```ts
Phase 0 measurement (you, on the Mac)                 ~0.5 day
Native port compiles + Phase 1 parity                  2–4 days
Build: cmake crate, metallib, feature flag, bundle,    2–3 days
       signing, CI Metal toolchain (Dax review)
Hex integration: id, Labs, MlxArtifact download and    3–5 days
       receipts, Transcriber::Mlx, engine/meeting/
       service sites, benchmark backend, docs, maps
Phase 2 benchmarks + signed-app smoke test             1–2 days
Total                                                  ~2–3 weeks on a Mac
```

## Open Questions

1. Will you run Phase 0 first and accept "use the GGUF on macOS" if MLX
   isn't clearly better? Without that agreement the port is a bet, not
   engineering.
2. Separate id plus Labs (recommended), or replace the GGUF row on macOS
   outright once verified? AGENTS.md requires Labs, off by default, for a
   new or requested feature either way.
3. int8-g64 only, or also offer FP16 (1.17 GB) to users? The plan pins FP16
   for parity only.
4. Should the default Hex build carry MLX (the `mlx` feature) once it
   passes, or stay in local and test builds? It costs DMG size and CI time
   for everyone else.
5. Do you have FLEURS kk/ru references, or should Phase 0 rely on your own
   recordings only? Your own clips are the better judge; FLEURS gives a
   number others can reproduce.
6. Is `ky`/`uz` support wanted in the picker, or only `ru`/`kk`? It changes
   nothing at run time (no language input) and only what Settings lists.
