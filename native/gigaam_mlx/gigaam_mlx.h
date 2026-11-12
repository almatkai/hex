/*
 * gigaam_mlx: GigaAM-Multilingual (CTC) on MLX, for Hex on Apple Silicon.
 *
 * SKELETON, UNTESTED. Written on Windows from the ai-babai
 * gigaam-multilingual-mlx 0.2.0 reference (model.py, service.py) without a
 * Mac; nothing here has been compiled. See docs/plans/gigaam-mlx.md.
 *
 * The library is a C ABI over MLX's C++ API (pinned v0.32.4). Rust owns
 * policy (catalog, download, SHA-256 receipts, config.json parsing,
 * chunk scheduling on the engine thread); this library owns compute.
 *
 * Threading: every entry point may be called from any thread, but all MLX
 * work is serialized by one process-wide mutex inside the library. Hex calls
 * it only from the transcription engine thread.
 *
 * Memory: strings and results returned here are owned by the caller and
 * must be released with the matching *_free function.
 */
#ifndef GIGAAM_MLX_H
#define GIGAAM_MLX_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GIGAAM_MLX_ABI_VERSION 1

typedef struct gigaam_mlx_session gigaam_mlx_session;
typedef struct gigaam_mlx_result gigaam_mlx_result;

typedef enum gigaam_mlx_status {
  GIGAAM_MLX_OK = 0,
  /* Bad argument (null pointer, wrong sizes, unsupported config). */
  GIGAAM_MLX_INVALID = 1,
  /* A file is missing, unreadable, or its SHA-256 does not match. */
  GIGAAM_MLX_ARTIFACT = 2,
  /* No Metal device, or the metallib could not be loaded. Hex never falls
   * back to the CPU (strict-Metal invariant). */
  GIGAAM_MLX_NO_METAL = 3,
  /* should_cancel returned nonzero between chunks or layer groups. */
  GIGAAM_MLX_CANCELED = 4,
  /* Anything MLX threw. The message says what. */
  GIGAAM_MLX_RUNTIME = 5,
} gigaam_mlx_status;

/*
 * The model's architecture, parsed and validated by the caller from the
 * artifact's config.json (Rust: serde). The library cross-checks every
 * field against the tensor shapes in model.safetensors and refuses a
 * mismatch. Values for the pinned artifacts (config.json sha256
 * 72c544ee...a174) are noted beside each field.
 */
typedef struct gigaam_mlx_config {
  uint32_t abi_version;       /* GIGAAM_MLX_ABI_VERSION */
  uint32_t sample_rate;       /* 16000 */
  uint32_t features;          /* 64 mel bins */
  uint32_t win_length;        /* 320 (20 ms) */
  uint32_t hop_length;        /* 160 (10 ms) */
  uint32_t n_fft;             /* 320 */
  uint32_t n_layers;          /* 24 */
  uint32_t d_model;           /* 1024 */
  uint32_t n_heads;           /* 16 (head_dim 64) */
  uint32_t subs_kernel_size;  /* 5 */
  uint32_t ff_expansion;      /* 4 (d_ff 4096) */
  uint32_t conv_kernel_size;  /* 5 */
  uint32_t pos_emb_base;      /* 5000 (rotary base; upstream pos_emb_max_len) */
  uint32_t num_classes;       /* 71 = vocabulary_count + 1 (blank last) */
  /* 0 = float16 (both pinned artifacts), 1 = float32, 2 = bfloat16. */
  uint32_t dtype;
  /* Weight-only affine quantization of every nn.Linear, from manifest.json
   * "quantization": bits 8 and group 64 for int8-g64; 0 and 0 for FP16.
   * Conv1d weights (subsampling, pointwise, depthwise, CTC head) are never
   * quantized. */
  uint32_t quant_bits;
  uint32_t quant_group_size;
  /* The CTC vocabulary in config order, one UTF-8 string per token (70 for
   * the pinned model: " ", "'", a-z, Russian, Kazakh letters). Blank is
   * index vocabulary_count. Copied at load. */
  const char *const *vocabulary;
  uint32_t vocabulary_count;
} gigaam_mlx_config;

typedef struct gigaam_mlx_load_options {
  /* Directory holding model.safetensors (config.json and manifest.json are
   * read by the caller). UTF-8, required. */
  const char *model_dir;
  /* Absolute path to mlx.metallib (Hex.app/Contents/Resources/mlx.metallib).
   * NULL lets MLX search beside the executable. */
  const char *metallib_path;
  /* Lowercase hex SHA-256 of model.safetensors. NULL skips hashing, which
   * Hex does after its own receipt check (transcription/models). The parity
   * tool passes it. */
  const char *weights_sha256;
  /* MLX buffer cache limit in bytes after load; 0 keeps MLX's default. */
  uint64_t cache_limit_bytes;
  /* Nonzero: run one 1 s silent forward after loading so Metal pipelines
   * are built before the first dictation. */
  int32_t prewarm;
} gigaam_mlx_load_options;

typedef struct gigaam_mlx_transcribe_options {
  /* Long-audio chunking, as gigaam-stt does: 20 s windows with 2 s overlap,
   * keeping words whose midpoint falls in the window minus half the overlap
   * at each inner edge. chunk_seconds 0 selects both defaults (20.0 and
   * 2.0); otherwise overlap_seconds is used as given. The upstream model
   * refuses more than 25 s in one pass. */
  float chunk_seconds;
  float overlap_seconds;
  /* Polled between chunks and between groups of layers; nonzero cancels.
   * A running Metal command buffer is never aborted. May be NULL. */
  int32_t (*should_cancel)(void *user);
  void *cancel_user;
  /* Nonzero: clear MLX's buffer cache after the call, so an idle Hex holds
   * the weights only. */
  int32_t release_cache;
} gigaam_mlx_transcribe_options;

/* One word: CTC emission times, approximate, not forced alignment. */
typedef struct gigaam_mlx_word {
  const char *text; /* UTF-8, valid until the result is freed */
  int64_t start_ms;
  int64_t end_ms;
} gigaam_mlx_word;

/* One emitted (non-blank, de-duplicated) token. */
typedef struct gigaam_mlx_token {
  uint32_t id;      /* index into the vocabulary */
  uint32_t frame;   /* encoder frame within its chunk (40 ms each) */
  int64_t time_ms;  /* absolute time of that frame in the clip */
} gigaam_mlx_token;

/* Library and MLX versions, e.g. "gigaam_mlx 1 (mlx 0.32.4)". Static. */
const char *gigaam_mlx_version(void);

/* Whether MLX sees a Metal GPU. */
int32_t gigaam_mlx_metal_available(void);

/* Loads the weights onto the GPU and evaluates them. On failure returns
 * NULL and, if error is not NULL, a message to free with
 * gigaam_mlx_string_free. */
gigaam_mlx_session *gigaam_mlx_load(const gigaam_mlx_config *config,
                                    const gigaam_mlx_load_options *options,
                                    gigaam_mlx_status *status,
                                    char **error);

/* Transcribes 16 kHz mono float PCM in [-1, 1]. Any length, including 0
 * (an empty result). Returns NULL on failure, with status and error. */
gigaam_mlx_result *gigaam_mlx_transcribe(gigaam_mlx_session *session,
                                         const float *samples,
                                         size_t sample_count,
                                         const gigaam_mlx_transcribe_options *options,
                                         gigaam_mlx_status *status,
                                         char **error);

/* Parity hook: one forward pass over the whole input with no chunking,
 * writing float32 CTC log-probabilities, frames x num_classes, row-major.
 * Call with out == NULL to learn *frames. */
gigaam_mlx_status gigaam_mlx_log_probs(gigaam_mlx_session *session,
                                       const float *samples,
                                       size_t sample_count,
                                       float *out,
                                       size_t out_capacity,
                                       size_t *frames,
                                       char **error);

const char *gigaam_mlx_result_text(const gigaam_mlx_result *result);
size_t gigaam_mlx_result_word_count(const gigaam_mlx_result *result);
gigaam_mlx_word gigaam_mlx_result_word(const gigaam_mlx_result *result, size_t index);
size_t gigaam_mlx_result_token_count(const gigaam_mlx_result *result);
gigaam_mlx_token gigaam_mlx_result_token(const gigaam_mlx_result *result, size_t index);
void gigaam_mlx_result_free(gigaam_mlx_result *result);

/* MLX allocator statistics in bytes (process-wide, not per session). */
void gigaam_mlx_memory(uint64_t *active, uint64_t *peak, uint64_t *cache);
void gigaam_mlx_reset_peak_memory(void);

void gigaam_mlx_free(gigaam_mlx_session *session);
void gigaam_mlx_string_free(char *string);

#ifdef __cplusplus
}
#endif

#endif /* GIGAAM_MLX_H */
