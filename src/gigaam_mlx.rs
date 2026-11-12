//! GigaAM-Multilingual on MLX (Apple Silicon): the safe Rust side of
//! `native/gigaam_mlx`.
//!
//! SKELETON, UNTESTED, NOT IN THE MODULE TREE. Nothing declares
//! `mod gigaam_mlx;` yet, so this file does not build with Hex today. Wiring
//! it in (docs/plans/gigaam-mlx.md, "Integration") means:
//! `#[cfg(all(target_os = "macos", target_arch = "aarch64"))] mod gigaam_mlx;`
//! in `main.rs`, `compile_gigaam_mlx()` in `build.rs`, and a
//! `Transcriber::Mlx` variant in `transcription.rs`.
//!
//! Rust owns policy: the pinned artifact, its download and SHA-256 receipts
//! (`transcription::models`), parsing `config.json` and `manifest.json`, and
//! calling in from the one engine thread. The C++ side owns the forward pass.

use std::ffi::{CStr, CString, c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, bail, eyre};
use serde::Deserialize;

use crate::transcription::transcript::{Transcript, TranscriptSegment};

const ABI_VERSION: u32 = 1;

// ------------------------------------------------------------------- FFI

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum Status {
    Ok = 0,
    Invalid = 1,
    Artifact = 2,
    NoMetal = 3,
    Canceled = 4,
    Runtime = 5,
}

#[repr(C)]
struct RawConfig {
    abi_version: u32,
    sample_rate: u32,
    features: u32,
    win_length: u32,
    hop_length: u32,
    n_fft: u32,
    n_layers: u32,
    d_model: u32,
    n_heads: u32,
    subs_kernel_size: u32,
    ff_expansion: u32,
    conv_kernel_size: u32,
    pos_emb_base: u32,
    num_classes: u32,
    dtype: u32,
    quant_bits: u32,
    quant_group_size: u32,
    vocabulary: *const *const c_char,
    vocabulary_count: u32,
}

#[repr(C)]
struct RawLoadOptions {
    model_dir: *const c_char,
    metallib_path: *const c_char,
    weights_sha256: *const c_char,
    cache_limit_bytes: u64,
    prewarm: i32,
}

#[repr(C)]
struct RawTranscribeOptions {
    chunk_seconds: f32,
    overlap_seconds: f32,
    should_cancel: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
    cancel_user: *mut c_void,
    release_cache: i32,
}

#[repr(C)]
struct RawWord {
    text: *const c_char,
    start_ms: i64,
    end_ms: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub id: u32,
    pub frame: u32,
    pub time_ms: i64,
}

#[repr(C)]
struct RawSession {
    _private: [u8; 0],
}

#[repr(C)]
struct RawResult {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn gigaam_mlx_metal_available() -> i32;
    fn gigaam_mlx_load(
        config: *const RawConfig,
        options: *const RawLoadOptions,
        status: *mut Status,
        error: *mut *mut c_char,
    ) -> *mut RawSession;
    fn gigaam_mlx_transcribe(
        session: *mut RawSession,
        samples: *const f32,
        sample_count: usize,
        options: *const RawTranscribeOptions,
        status: *mut Status,
        error: *mut *mut c_char,
    ) -> *mut RawResult;
    fn gigaam_mlx_log_probs(
        session: *mut RawSession,
        samples: *const f32,
        sample_count: usize,
        out: *mut f32,
        out_capacity: usize,
        frames: *mut usize,
        error: *mut *mut c_char,
    ) -> Status;
    fn gigaam_mlx_result_text(result: *const RawResult) -> *const c_char;
    fn gigaam_mlx_result_word_count(result: *const RawResult) -> usize;
    fn gigaam_mlx_result_word(result: *const RawResult, index: usize) -> RawWord;
    fn gigaam_mlx_result_token_count(result: *const RawResult) -> usize;
    fn gigaam_mlx_result_token(result: *const RawResult, index: usize) -> Token;
    fn gigaam_mlx_result_free(result: *mut RawResult);
    fn gigaam_mlx_memory(active: *mut u64, peak: *mut u64, cache: *mut u64);
    fn gigaam_mlx_free(session: *mut RawSession);
    fn gigaam_mlx_string_free(string: *mut c_char);
}

// ------------------------------------------------- config.json, manifest

/// The artifact's `config.json` (gigaam_multilingual_mlx.config.ModelConfig).
#[derive(Clone, Debug, Deserialize)]
pub struct ModelConfig {
    pub model_name: String,
    pub source_revision: String,
    pub sample_rate: u32,
    pub features: u32,
    pub win_length: u32,
    pub hop_length: u32,
    pub n_fft: u32,
    pub center: bool,
    pub n_layers: u32,
    pub d_model: u32,
    pub subs_kernel_size: u32,
    pub subsampling_factor: u32,
    pub ff_expansion_factor: u32,
    pub n_heads: u32,
    pub pos_emb_base: u32,
    pub conv_kernel_size: u32,
    pub num_classes: u32,
    pub vocabulary: Vec<String>,
    #[serde(default = "float32")]
    pub dtype: String,
}

fn float32() -> String {
    "float32".into()
}

#[derive(Clone, Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    architecture: String,
    #[serde(default)]
    quantization: Option<Quantization>,
}

#[derive(Clone, Debug, Deserialize)]
struct Quantization {
    method: String,
    group_size: u32,
    rules: Vec<QuantizationRule>,
}

#[derive(Clone, Debug, Deserialize)]
struct QuantizationRule {
    selector: String,
    bits: u32,
}

const ARCHITECTURE: &str = "gigaam_multilingual_ctc_conv1d_rotary_conformer";

impl ModelConfig {
    /// Refuses anything but the one architecture the C++ port implements.
    /// The pinned artifacts' config.json is byte-identical across FP16 and
    /// int8-g64 (sha256 72c544ee...a174), so this is belt and braces.
    fn validate(&self) -> Result<()> {
        let expected = (16_000, 64, 320, 160, 320, false, 24, 1024, 5, 4, 4, 16, 5000, 5);
        let actual = (
            self.sample_rate,
            self.features,
            self.win_length,
            self.hop_length,
            self.n_fft,
            self.center,
            self.n_layers,
            self.d_model,
            self.subs_kernel_size,
            self.subsampling_factor,
            self.ff_expansion_factor,
            self.n_heads,
            self.pos_emb_base,
            self.conv_kernel_size,
        );
        if actual != expected {
            bail!("unsupported GigaAM MLX configuration {actual:?}");
        }
        if self.num_classes as usize != self.vocabulary.len() + 1 {
            bail!("GigaAM MLX vocabulary does not match its class count");
        }
        if self.vocabulary.first().map(String::as_str) != Some(" ") {
            bail!("GigaAM MLX vocabulary must start with the word separator");
        }
        Ok(())
    }

    fn dtype_code(&self) -> Result<u32> {
        Ok(match self.dtype.as_str() {
            "float16" => 0,
            "float32" => 1,
            "bfloat16" => 2,
            other => bail!("unsupported GigaAM MLX dtype {other}"),
        })
    }
}

/// `(bits, group_size)` for the C side: `(0, 0)` for FP16.
fn quantization(manifest: &Manifest) -> Result<(u32, u32)> {
    if manifest.schema_version != 1 || manifest.architecture != ARCHITECTURE {
        bail!("unsupported GigaAM MLX manifest");
    }
    let Some(quantization) = &manifest.quantization else {
        return Ok((0, 0));
    };
    // Only the published profiles' shape: one all_linear rule.
    match quantization.rules.as_slice() {
        [rule] if quantization.method == "weight_only_affine" && rule.selector == "all_linear" => {
            Ok((rule.bits, quantization.group_size))
        }
        _ => bail!("unsupported GigaAM MLX quantization"),
    }
}

// --------------------------------------------------------------- session

pub struct GigaAmMlx {
    session: NonNull<RawSession>,
    vocabulary: Vec<String>,
}

// The session is used only from the transcription engine thread, and the
// library serializes every MLX call behind its own mutex.
unsafe impl Send for GigaAmMlx {}

impl GigaAmMlx {
    pub fn metal_available() -> bool {
        unsafe { gigaam_mlx_metal_available() != 0 }
    }

    /// Loads a verified artifact directory (config.json, manifest.json,
    /// model.safetensors). `weights_sha256` is `None` when Hex's receipt
    /// already vouches for the file; the parity tool passes the pin.
    pub fn load(model_dir: &Path, weights_sha256: Option<&str>) -> Result<Self> {
        let config: ModelConfig = serde_json::from_slice(&fs::read(model_dir.join("config.json"))?)?;
        config.validate()?;
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(model_dir.join("manifest.json"))?)?;
        let (quant_bits, quant_group_size) = quantization(&manifest)?;

        let vocabulary = config
            .vocabulary
            .iter()
            .map(|piece| CString::new(piece.as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let pointers: Vec<*const c_char> = vocabulary.iter().map(|piece| piece.as_ptr()).collect();
        let raw = RawConfig {
            abi_version: ABI_VERSION,
            sample_rate: config.sample_rate,
            features: config.features,
            win_length: config.win_length,
            hop_length: config.hop_length,
            n_fft: config.n_fft,
            n_layers: config.n_layers,
            d_model: config.d_model,
            n_heads: config.n_heads,
            subs_kernel_size: config.subs_kernel_size,
            ff_expansion: config.ff_expansion_factor,
            conv_kernel_size: config.conv_kernel_size,
            pos_emb_base: config.pos_emb_base,
            num_classes: config.num_classes,
            dtype: config.dtype_code()?,
            quant_bits,
            quant_group_size,
            vocabulary: pointers.as_ptr(),
            vocabulary_count: u32::try_from(pointers.len())?,
        };
        let dir = path_cstring(model_dir)?;
        let metallib = metallib_path().map(|path| path_cstring(&path)).transpose()?;
        let sha = weights_sha256.map(CString::new).transpose()?;
        let options = RawLoadOptions {
            model_dir: dir.as_ptr(),
            metallib_path: metallib.as_ref().map_or(ptr::null(), |path| path.as_ptr()),
            weights_sha256: sha.as_ref().map_or(ptr::null(), |sha| sha.as_ptr()),
            // Keep MLX's free-buffer cache small: Hex idles most of the day.
            cache_limit_bytes: 256 << 20,
            prewarm: 1,
        };
        let mut status = Status::Ok;
        let mut error = ptr::null_mut();
        let session = unsafe { gigaam_mlx_load(&raw, &options, &mut status, &mut error) };
        let session = NonNull::new(session).ok_or_else(|| take_error(status, error))?;
        Ok(Self {
            session,
            vocabulary: config.vocabulary,
        })
    }

    /// Transcribes 16 kHz mono audio, with word times from CTC emissions.
    /// Returns `Err` naming cancellation when `cancel` was set between
    /// chunks or layer groups.
    pub fn transcribe(&mut self, samples: &[f32], cancel: &AtomicBool) -> Result<Transcript> {
        let (transcript, _) = self.transcribe_with_tokens(samples, cancel)?;
        Ok(transcript)
    }

    pub fn transcribe_with_tokens(
        &mut self,
        samples: &[f32],
        cancel: &AtomicBool,
    ) -> Result<(Transcript, Vec<Token>)> {
        unsafe extern "C" fn should_cancel(user: *mut c_void) -> i32 {
            let flag = unsafe { &*(user as *const AtomicBool) };
            i32::from(flag.load(Ordering::Acquire))
        }
        let options = RawTranscribeOptions {
            chunk_seconds: 0.0, // gigaam-stt's 20 s / 2 s
            overlap_seconds: 0.0,
            should_cancel: Some(should_cancel),
            cancel_user: ptr::from_ref(cancel).cast_mut().cast(),
            release_cache: 1,
        };
        let mut status = Status::Ok;
        let mut error = ptr::null_mut();
        let result = unsafe {
            gigaam_mlx_transcribe(
                self.session.as_ptr(),
                samples.as_ptr(),
                samples.len(),
                &options,
                &mut status,
                &mut error,
            )
        };
        let result = NonNull::new(result).ok_or_else(|| take_error(status, error))?;
        let raw = result.as_ptr().cast_const();
        let text = unsafe { CStr::from_ptr(gigaam_mlx_result_text(raw)) }
            .to_string_lossy()
            .into_owned();
        let words = (0..unsafe { gigaam_mlx_result_word_count(raw) })
            .map(|index| {
                let word = unsafe { gigaam_mlx_result_word(raw, index) };
                TranscriptSegment {
                    start_ms: word.start_ms,
                    end_ms: word.end_ms,
                    text: unsafe { CStr::from_ptr(word.text) }
                        .to_string_lossy()
                        .into_owned(),
                }
            })
            .collect::<Vec<_>>();
        let tokens = (0..unsafe { gigaam_mlx_result_token_count(raw) })
            .map(|index| unsafe { gigaam_mlx_result_token(raw, index) })
            .collect();
        unsafe { gigaam_mlx_result_free(result.as_ptr()) };
        // One segment spanning the words: the model has no sentence breaks.
        let segments = match (words.first(), words.last()) {
            (Some(first), Some(last)) => vec![TranscriptSegment {
                start_ms: first.start_ms,
                end_ms: last.end_ms,
                text: text.clone(),
            }],
            _ => Vec::new(),
        };
        Ok((
            Transcript {
                text,
                segments,
                words,
            },
            tokens,
        ))
    }

    /// Parity: float32 CTC log-probabilities over the whole input, one
    /// pass, `frames x (vocabulary + 1)` row-major.
    pub fn log_probs(&mut self, samples: &[f32]) -> Result<(usize, Vec<f32>)> {
        let classes = self.vocabulary.len() + 1;
        let mut frames = 0;
        let mut error = ptr::null_mut();
        let status = unsafe {
            gigaam_mlx_log_probs(
                self.session.as_ptr(),
                samples.as_ptr(),
                samples.len(),
                ptr::null_mut(),
                0,
                &mut frames,
                &mut error,
            )
        };
        if status != Status::Ok {
            return Err(take_error(status, error));
        }
        // ASSUMPTION: the sizing call runs the forward once; the second runs
        // it again. Fine for a parity tool, not for the hot path.
        let mut out = vec![0.0f32; frames * classes];
        let status = unsafe {
            gigaam_mlx_log_probs(
                self.session.as_ptr(),
                samples.as_ptr(),
                samples.len(),
                out.as_mut_ptr(),
                out.len(),
                &mut frames,
                &mut error,
            )
        };
        if status != Status::Ok {
            return Err(take_error(status, error));
        }
        Ok((frames, out))
    }

    pub fn vocabulary(&self) -> &[String] {
        &self.vocabulary
    }

    /// MLX allocator bytes: (active, peak, cache), process-wide.
    pub fn memory() -> (u64, u64, u64) {
        let (mut active, mut peak, mut cache) = (0, 0, 0);
        unsafe { gigaam_mlx_memory(&mut active, &mut peak, &mut cache) };
        (active, peak, cache)
    }
}

impl Drop for GigaAmMlx {
    fn drop(&mut self) {
        unsafe { gigaam_mlx_free(self.session.as_ptr()) };
    }
}

/// `Hex.app/Contents/Resources/mlx.metallib` beside a bundled executable,
/// else the one build.rs left in OUT_DIR for `cargo run`.
fn metallib_path() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .join("Resources/mlx.metallib");
    if bundled.is_file() {
        return Some(bundled);
    }
    option_env!("HEX_MLX_METALLIB")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn path_cstring(path: &Path) -> Result<CString> {
    let text = path
        .to_str()
        .ok_or_else(|| eyre!("{} is not UTF-8", path.display()))?;
    Ok(CString::new(text)?)
}

fn take_error(status: Status, error: *mut c_char) -> color_eyre::Report {
    let message = if error.is_null() {
        "GigaAM MLX failed without a message".to_owned()
    } else {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { gigaam_mlx_string_free(error) };
        message
    };
    match status {
        Status::Canceled => eyre!("transcription canceled"),
        Status::NoMetal => eyre!("GigaAM MLX needs a Metal GPU: {message}"),
        Status::Artifact => eyre!("GigaAM MLX model files are damaged: {message}"),
        _ => eyre!("GigaAM MLX: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinned int8-g64 config.json, vocabulary shortened for the test
    /// (the real one has 70 entries and num_classes 71).
    const CONFIG: &str = r#"{
      "model_name": "multilingual_large_ctc",
      "source_revision": "3905cd51c3ed4e88c8edf33f3302969ba480a327",
      "sample_rate": 16000, "features": 64, "win_length": 320,
      "hop_length": 160, "n_fft": 320, "center": false, "n_layers": 24,
      "d_model": 1024, "subs_kernel_size": 5, "subsampling_factor": 4,
      "ff_expansion_factor": 4, "n_heads": 16, "pos_emb_base": 5000,
      "conv_kernel_size": 5, "num_classes": 4,
      "vocabulary": [" ", "'", "қ"], "dtype": "float16"
    }"#;

    #[test]
    fn accepts_the_pinned_architecture() {
        let config: ModelConfig = serde_json::from_str(CONFIG).unwrap();
        config.validate().unwrap();
        assert_eq!(config.dtype_code().unwrap(), 0);
    }

    #[test]
    fn reads_int8_g64_from_the_manifest() {
        let manifest: Manifest = serde_json::from_str(&format!(
            r#"{{"schema_version":1,"architecture":"{ARCHITECTURE}","quantization":
            {{"profile":"int8-g64","method":"weight_only_affine","group_size":64,
            "rules":[{{"selector":"all_linear","bits":8}}]}}}}"#
        ))
        .unwrap();
        assert_eq!(quantization(&manifest).unwrap(), (8, 64));
    }

    #[test]
    fn fp16_manifest_has_no_quantization() {
        let manifest: Manifest = serde_json::from_str(&format!(
            r#"{{"schema_version":1,"architecture":"{ARCHITECTURE}","dtype":"float16"}}"#
        ))
        .unwrap();
        assert_eq!(quantization(&manifest).unwrap(), (0, 0));
    }
}
