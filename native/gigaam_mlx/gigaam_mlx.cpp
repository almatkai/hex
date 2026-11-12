// gigaam_mlx: GigaAM-Multilingual CTC forward pass on MLX's C++ API.
//
// SKELETON, UNTESTED, NEVER COMPILED. Ported line by line from
// gigaam-multilingual-mlx 0.2.0 (src/gigaam_multilingual_mlx/model.py and
// service.py), which itself mirrors salute-developers/GigaAM's
// encoder.py. Every "ASSUMPTION" comment marks something to confirm on a
// Mac; docs/plans/gigaam-mlx.md has the verification plan.
//
// Target: MLX v0.32.4 headers (signatures checked against that tag's
// mlx/ops.h, fast.h, fft.h, io.h, memory.h, backend/metal/metal.h).
// Requires C++20 (designated initializers).

#include "gigaam_mlx.h"

#include <CommonCrypto/CommonDigest.h>

#include <mlx/backend/metal/metal.h>
#include <mlx/mlx.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <type_traits>
#include <unordered_map>
#include <utility>
#include <vector>

namespace mx = mlx::core;

namespace {

// ---------------------------------------------------------------- plumbing

// MLX's default stream is process-global; Hex runs one engine thread, but
// the local API's host-audio service can load a second transcriber, so all
// MLX work goes through one lock.
std::mutex& mlx_lock() {
  static std::mutex lock;
  return lock;
}

struct Failure {
  gigaam_mlx_status status;
  std::string message;
};

[[noreturn]] void fail(gigaam_mlx_status status, std::string message) {
  throw Failure{status, std::move(message)};
}

char* duplicate(const std::string& text) {
  auto* copy = static_cast<char*>(std::malloc(text.size() + 1));
  if (copy != nullptr) {
    std::memcpy(copy, text.data(), text.size());
    copy[text.size()] = '\0';
  }
  return copy;
}

void report(gigaam_mlx_status* status, char** error, gigaam_mlx_status code,
            const std::string& message) {
  if (status != nullptr) *status = code;
  if (error != nullptr) *error = duplicate(message);
}

// Runs `body`, translating exceptions into a status and message.
template <typename F>
auto guarded(gigaam_mlx_status* status, char** error, F&& body)
    -> decltype(body()) {
  using R = decltype(body());
  if (status != nullptr) *status = GIGAAM_MLX_OK;
  if (error != nullptr) *error = nullptr;
  try {
    return body();
  } catch (const Failure& failure) {
    report(status, error, failure.status, failure.message);
  } catch (const std::exception& exception) {
    report(status, error, GIGAAM_MLX_RUNTIME, exception.what());
  } catch (...) {
    report(status, error, GIGAAM_MLX_RUNTIME, "unknown MLX failure");
  }
  if constexpr (std::is_pointer_v<R>) {
    return nullptr;
  } else {
    return R{};
  }
}

std::string sha256_file(const std::string& path) {
  std::ifstream file(path, std::ios::binary);
  if (!file) fail(GIGAAM_MLX_ARTIFACT, "cannot read " + path);
  CC_SHA256_CTX context;
  CC_SHA256_Init(&context);
  std::vector<char> buffer(8 << 20);
  while (file) {
    file.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
    auto got = file.gcount();
    if (got > 0) CC_SHA256_Update(&context, buffer.data(), static_cast<CC_LONG>(got));
  }
  unsigned char digest[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256_Final(digest, &context);
  char hex[CC_SHA256_DIGEST_LENGTH * 2 + 1];
  for (int i = 0; i < CC_SHA256_DIGEST_LENGTH; ++i) {
    std::snprintf(hex + i * 2, 3, "%02x", digest[i]);
  }
  return std::string(hex, CC_SHA256_DIGEST_LENGTH * 2);
}

std::string shape_text(const mx::Shape& shape) {
  std::string text = "[";
  for (size_t i = 0; i < shape.size(); ++i) {
    if (i) text += ",";
    text += std::to_string(shape[i]);
  }
  return text + "]";
}

// A scalar in the graph's dtype. Python's `0.5 * x` keeps an fp16 x in fp16
// (weakly typed scalars); C++'s `x * 0.5f` would promote to float32.
mx::array scalar(float value, mx::Dtype dtype) { return mx::array(value, dtype); }

// ------------------------------------------------------------------ weights

struct Linear {
  mx::array weight;  // [out, in] float16, or [out, in*bits/32] uint32
  mx::array bias;    // [out]
  std::optional<mx::array> scales;  // [out, in/group] when quantized
  std::optional<mx::array> biases;  // [out, in/group] when quantized
  int bits;
  int group;
};

struct Conv {
  mx::array weight;  // MLX layout [out, kernel, in/groups]
  mx::array bias;    // [out]
};

struct Norm {
  mx::array weight;
  mx::array bias;
};

struct Layer {
  Norm norm_feed_forward1;
  Linear ff1_linear1, ff1_linear2;
  Norm norm_self_att;
  Linear linear_q, linear_k, linear_v, linear_out;
  Norm norm_conv;
  Conv pointwise_conv1, depthwise_conv;
  Norm conv_norm;  // the checkpoint calls it batch_norm; it is a LayerNorm
  Conv pointwise_conv2;
  Norm norm_feed_forward2;
  Linear ff2_linear1, ff2_linear2;
  Norm norm_out;
};

// Takes each tensor out of the loaded map once, checking shape and dtype,
// so whatever is left at the end is an unexpected tensor (strict load, as
// Python's load_weights(strict=True)).
class Loader {
 public:
  explicit Loader(std::unordered_map<std::string, mx::array> tensors)
      : tensors_(std::move(tensors)) {}

  mx::array take(const std::string& name, const mx::Shape& shape, mx::Dtype dtype) {
    auto found = tensors_.find(name);
    if (found == tensors_.end()) fail(GIGAAM_MLX_ARTIFACT, "missing tensor " + name);
    mx::array tensor = found->second;
    tensors_.erase(found);
    if (tensor.shape() != shape) {
      fail(GIGAAM_MLX_ARTIFACT, name + " has shape " + shape_text(tensor.shape()) +
                                    ", expected " + shape_text(shape));
    }
    if (tensor.dtype() != dtype) fail(GIGAAM_MLX_ARTIFACT, name + " has an unexpected dtype");
    return tensor;
  }

  void expect_empty() const {
    if (!tensors_.empty()) {
      fail(GIGAAM_MLX_ARTIFACT, "unexpected tensor " + tensors_.begin()->first);
    }
  }

 private:
  std::unordered_map<std::string, mx::array> tensors_;
};

mx::Dtype compute_dtype(uint32_t code) {
  switch (code) {
    case 0: return mx::float16;
    case 1: return mx::float32;
    case 2: return mx::bfloat16;
    default: fail(GIGAAM_MLX_INVALID, "unknown dtype code " + std::to_string(code));
  }
}

// ---------------------------------------------------------------- the model

struct Model {
  gigaam_mlx_config config{};
  std::vector<std::string> vocabulary;
  mx::Dtype dtype = mx::float16;

  // Frontend: torchaudio MelSpectrogram's window and filterbank, stored as
  // float32 tensors in the artifact (no need to recompute either).
  std::optional<mx::array> window;      // [win_length]
  std::optional<mx::array> mel_filter;  // [n_fft/2 + 1, features]
  std::optional<Conv> subsample1, subsample2;
  std::vector<Layer> layers;
  std::optional<Conv> head;

  int head_dim() const { return static_cast<int>(config.d_model / config.n_heads); }
  int blank() const { return static_cast<int>(vocabulary.size()); }
};

Norm take_norm(Loader& loader, const std::string& prefix, int d, mx::Dtype dtype) {
  return Norm{loader.take(prefix + ".weight", {d}, dtype), loader.take(prefix + ".bias", {d}, dtype)};
}

Conv take_conv(Loader& loader, const std::string& prefix, int out, int kernel, int in,
               mx::Dtype dtype) {
  return Conv{loader.take(prefix + ".weight", {out, kernel, in}, dtype),
              loader.take(prefix + ".bias", {out}, dtype)};
}

Linear take_linear(Loader& loader, const std::string& prefix, int out, int in,
                   const gigaam_mlx_config& config, mx::Dtype dtype) {
  int bits = static_cast<int>(config.quant_bits);
  int group = static_cast<int>(config.quant_group_size);
  if (bits == 0) {
    return Linear{loader.take(prefix + ".weight", {out, in}, dtype),
                  loader.take(prefix + ".bias", {out}, dtype), std::nullopt, std::nullopt, 0, 0};
  }
  if (in % group != 0 || (in * bits) % 32 != 0) {
    fail(GIGAAM_MLX_INVALID, prefix + " cannot be quantized with this group or width");
  }
  // MLX affine packing (checked against the int8-g64 header: linear1.weight
  // U32 [4096,256], scales and biases F16 [4096,16]).
  return Linear{loader.take(prefix + ".weight", {out, in * bits / 32}, mx::uint32),
                loader.take(prefix + ".bias", {out}, dtype),
                loader.take(prefix + ".scales", {out, in / group}, dtype),
                loader.take(prefix + ".biases", {out, in / group}, dtype), bits, group};
}

void validate_config(const gigaam_mlx_config& c) {
  if (c.abi_version != GIGAAM_MLX_ABI_VERSION) fail(GIGAAM_MLX_INVALID, "ABI version mismatch");
  if (c.sample_rate != 16000) fail(GIGAAM_MLX_INVALID, "only 16 kHz is supported");
  if (c.n_heads == 0 || c.d_model % c.n_heads != 0) fail(GIGAAM_MLX_INVALID, "bad head count");
  if ((c.d_model / c.n_heads) % 2 != 0) fail(GIGAAM_MLX_INVALID, "rotary needs an even head size");
  if (c.subs_kernel_size % 2 == 0 || c.conv_kernel_size % 2 == 0) {
    fail(GIGAAM_MLX_INVALID, "kernels must be odd");
  }
  if (c.n_fft < c.win_length || c.hop_length == 0) fail(GIGAAM_MLX_INVALID, "bad frontend");
  if (c.vocabulary == nullptr || c.vocabulary_count + 1 != c.num_classes) {
    fail(GIGAAM_MLX_INVALID, "num_classes must be the vocabulary plus blank");
  }
  if ((c.quant_bits == 0) != (c.quant_group_size == 0)) {
    fail(GIGAAM_MLX_INVALID, "quant_bits and quant_group_size go together");
  }
}

std::unique_ptr<Model> load_model(const gigaam_mlx_config& config,
                                  const gigaam_mlx_load_options& options) {
  validate_config(config);
  if (options.model_dir == nullptr) fail(GIGAAM_MLX_INVALID, "model_dir is required");
  std::string weights = std::string(options.model_dir) + "/model.safetensors";
  if (options.weights_sha256 != nullptr && sha256_file(weights) != options.weights_sha256) {
    fail(GIGAAM_MLX_ARTIFACT, "model.safetensors does not match its pinned SHA-256");
  }

  auto model = std::make_unique<Model>();
  model->config = config;
  model->dtype = compute_dtype(config.dtype);
  for (uint32_t i = 0; i < config.vocabulary_count; ++i) {
    model->vocabulary.emplace_back(config.vocabulary[i]);
  }
  model->config.vocabulary = nullptr;  // the strings are ours now

  // load_safetensors maps lazily; mx::eval below reads it onto the GPU.
  auto [tensors, metadata] = mx::load_safetensors(weights);
  (void)metadata;  // ASSUMPTION: "format" is "gigaam-multilingual-mlx"; unchecked.
  Loader loader(std::move(tensors));
  const int d = static_cast<int>(config.d_model);
  const int ff = d * static_cast<int>(config.ff_expansion);
  const int sk = static_cast<int>(config.subs_kernel_size);
  const int ck = static_cast<int>(config.conv_kernel_size);
  const mx::Dtype dt = model->dtype;

  model->window = loader.take("preprocessor.window", {static_cast<int>(config.win_length)}, mx::float32);
  model->mel_filter = loader.take("preprocessor.mel_filter",
                                  {static_cast<int>(config.n_fft / 2 + 1), static_cast<int>(config.features)},
                                  mx::float32);
  model->subsample1 = take_conv(loader, "encoder.pre_encode.conv1", d, sk, static_cast<int>(config.features), dt);
  model->subsample2 = take_conv(loader, "encoder.pre_encode.conv2", d, sk, d, dt);
  model->layers.reserve(config.n_layers);
  for (uint32_t i = 0; i < config.n_layers; ++i) {
    const std::string p = "encoder.layers." + std::to_string(i) + ".";
    model->layers.push_back(Layer{
        .norm_feed_forward1 = take_norm(loader, p + "norm_feed_forward1", d, dt),
        .ff1_linear1 = take_linear(loader, p + "feed_forward1.linear1", ff, d, config, dt),
        .ff1_linear2 = take_linear(loader, p + "feed_forward1.linear2", d, ff, config, dt),
        .norm_self_att = take_norm(loader, p + "norm_self_att", d, dt),
        .linear_q = take_linear(loader, p + "self_attn.linear_q", d, d, config, dt),
        .linear_k = take_linear(loader, p + "self_attn.linear_k", d, d, config, dt),
        .linear_v = take_linear(loader, p + "self_attn.linear_v", d, d, config, dt),
        .linear_out = take_linear(loader, p + "self_attn.linear_out", d, d, config, dt),
        .norm_conv = take_norm(loader, p + "norm_conv", d, dt),
        .pointwise_conv1 = take_conv(loader, p + "conv.pointwise_conv1", 2 * d, 1, d, dt),
        .depthwise_conv = take_conv(loader, p + "conv.depthwise_conv", d, ck, 1, dt),
        .conv_norm = take_norm(loader, p + "conv.batch_norm", d, dt),
        .pointwise_conv2 = take_conv(loader, p + "conv.pointwise_conv2", d, 1, d, dt),
        .norm_feed_forward2 = take_norm(loader, p + "norm_feed_forward2", d, dt),
        .ff2_linear1 = take_linear(loader, p + "feed_forward2.linear1", ff, d, config, dt),
        .ff2_linear2 = take_linear(loader, p + "feed_forward2.linear2", d, ff, config, dt),
        .norm_out = take_norm(loader, p + "norm_out", d, dt),
    });
  }
  model->head = take_conv(loader, "head.decoder", static_cast<int>(config.num_classes), 1, d, dt);
  loader.expect_empty();  // 824 tensors (FP16) or 1208 (int8-g64), all consumed

  // Read every weight onto the device now, not on the first dictation.
  std::vector<mx::array> all{*model->window, *model->mel_filter};
  auto add_conv = [&](const Conv& c) { all.push_back(c.weight); all.push_back(c.bias); };
  auto add_norm = [&](const Norm& n) { all.push_back(n.weight); all.push_back(n.bias); };
  auto add_linear = [&](const Linear& l) {
    all.push_back(l.weight);
    all.push_back(l.bias);
    if (l.scales) all.push_back(*l.scales);
    if (l.biases) all.push_back(*l.biases);
  };
  add_conv(*model->subsample1);
  add_conv(*model->subsample2);
  for (const auto& l : model->layers) {
    add_norm(l.norm_feed_forward1); add_linear(l.ff1_linear1); add_linear(l.ff1_linear2);
    add_norm(l.norm_self_att); add_linear(l.linear_q); add_linear(l.linear_k);
    add_linear(l.linear_v); add_linear(l.linear_out); add_norm(l.norm_conv);
    add_conv(l.pointwise_conv1); add_conv(l.depthwise_conv); add_norm(l.conv_norm);
    add_conv(l.pointwise_conv2); add_norm(l.norm_feed_forward2);
    add_linear(l.ff2_linear1); add_linear(l.ff2_linear2); add_norm(l.norm_out);
  }
  add_conv(*model->head);
  mx::eval(all);
  return model;
}

// ------------------------------------------------------------ forward pass

mx::array linear(const Linear& l, const mx::array& x) {
  if (l.scales) {
    // QuantizedLinear.__call__: quantized_matmul(transpose=True) + bias.
    auto y = mx::quantized_matmul(x, l.weight, *l.scales, l.biases, /*transpose=*/true,
                                  l.group, l.bits, "affine");
    return mx::add(y, l.bias);
  }
  // nn.Linear.__call__: addmm(bias, x, weight.T).
  return mx::addmm(l.bias, x, mx::transpose(l.weight));
}

mx::array conv(const Conv& c, const mx::array& x, int stride, int padding, int groups) {
  // nn.Conv1d.__call__ on NLC input: conv1d(...) + bias.
  return mx::add(mx::conv1d(x, c.weight, stride, padding, /*dilation=*/1, groups), c.bias);
}

mx::array norm(const Norm& n, const mx::array& x) {
  // nn.LayerNorm default eps 1e-5, affine; torch's default is the same.
  return mx::fast::layer_norm(x, n.weight, n.bias, 1e-5f);
}

mx::array silu(const mx::array& x) { return mx::multiply(x, mx::sigmoid(x)); }

mx::array relu(const mx::array& x) { return mx::maximum(x, scalar(0.0f, x.dtype())); }

mx::array feed_forward(const Linear& first, const Linear& second, const mx::array& x) {
  return linear(second, silu(linear(first, x)));
}

// [-x2, x1] for x = [x1, x2] split in half on the last axis.
mx::array rotate_half(const mx::array& x) {
  auto halves = mx::split(x, 2, -1);
  return mx::concatenate({mx::negative(halves[1]), halves[0]}, -1);
}

// GigaAM's RotaryPositionMultiHeadAttention: the rotation is applied to the
// layer input (split into heads) BEFORE the q and k projections, not to q
// and k after them; v uses the unrotated input. Batch 1, so no mask.
mx::array attention(const Layer& l, const mx::array& x, const mx::array& cos, const mx::array& sin,
                    int heads, int head_dim) {
  const int t = x.shape(1);
  const int width = x.shape(2);
  auto shaped = mx::reshape(x, {1, t, heads, head_dim});
  auto c = mx::reshape(cos, {1, t, 1, head_dim});
  auto s = mx::reshape(sin, {1, t, 1, head_dim});
  auto rotated = mx::add(mx::multiply(shaped, c), mx::multiply(rotate_half(shaped), s));
  auto flat = mx::reshape(rotated, {1, t, width});
  auto split_heads = [&](const mx::array& y) {
    return mx::transpose(mx::reshape(y, {1, t, heads, head_dim}), {0, 2, 1, 3});
  };
  auto q = split_heads(linear(l.linear_q, flat));
  auto k = split_heads(linear(l.linear_k, flat));
  auto v = split_heads(linear(l.linear_v, x));
  auto scale = scalar(1.0f / std::sqrt(static_cast<float>(head_dim)), q.dtype());  // 0.125
  auto scores = mx::multiply(mx::matmul(q, mx::transpose(k, {0, 1, 3, 2})), scale);
  // mx.softmax(axis=-1) with precise=False, as the reference calls it.
  // ASSUMPTION: an fp16 softmax matches; try precise=true if parity drifts.
  auto probabilities = mx::softmax(scores, -1);
  auto out = mx::reshape(mx::transpose(mx::matmul(probabilities, v), {0, 2, 1, 3}), {1, t, width});
  return linear(l.linear_out, out);
}

mx::array conv_module(const Layer& l, const mx::array& x, int kernel) {
  const int d = x.shape(2);
  auto y = conv(l.pointwise_conv1, x, 1, 0, 1);
  auto halves = mx::split(y, 2, -1);  // GLU over channels
  y = mx::multiply(halves[0], mx::sigmoid(halves[1]));
  // The padding mask is all-valid at batch 1, so it is skipped.
  y = conv(l.depthwise_conv, y, 1, (kernel - 1) / 2, /*groups=*/d);
  y = norm(l.conv_norm, y);
  return conv(l.pointwise_conv2, silu(y), 1, 0, 1);
}

mx::array conformer_layer(const Layer& l, const mx::array& x, const mx::array& cos,
                          const mx::array& sin, const gigaam_mlx_config& config, int head_dim) {
  auto half = scalar(0.5f, x.dtype());
  auto r = mx::add(x, mx::multiply(half, feed_forward(l.ff1_linear1, l.ff1_linear2,
                                                       norm(l.norm_feed_forward1, x))));
  r = mx::add(r, attention(l, norm(l.norm_self_att, r), cos, sin,
                           static_cast<int>(config.n_heads), head_dim));
  r = mx::add(r, conv_module(l, norm(l.norm_conv, r), static_cast<int>(config.conv_kernel_size)));
  r = mx::add(r, mx::multiply(half, feed_forward(l.ff2_linear1, l.ff2_linear2,
                                                 norm(l.norm_feed_forward2, r))));
  return norm(l.norm_out, r);
}

int frame_count(size_t samples, const gigaam_mlx_config& c) {
  // LogMelFrontend lengths: floor((len - win) / hop + 1), clamped at 0. Any
  // clip shorter than one window decodes to nothing.
  if (samples < c.win_length) return 0;
  return static_cast<int>(1 + (samples - c.win_length) / c.hop_length);
}

int subsampled_length(int length, int kernel) {
  // _length_after_conv with stride 2 and padding (kernel-1)/2, in float as
  // Python computes it.
  const double padding = (kernel - 1) / 2;
  return static_cast<int>(std::floor((length + 2 * padding - kernel) / 2.0 + 1.0));
}

struct Forward {
  mx::array log_probs;  // [1, T, num_classes] in the compute dtype
  int encoded_length;
};

// The whole model over one chunk. `cancel` is polled between layer groups.
std::optional<Forward> forward(const Model& m, const float* samples, size_t count,
                               const gigaam_mlx_transcribe_options* options) {
  const auto& c = m.config;
  const int features_length = frame_count(count, c);
  if (features_length == 0) return std::nullopt;

  // -- log-mel frontend (float32, center=False) --
  auto audio = mx::array(samples, {1, static_cast<int>(count)}, mx::float32);
  auto frames = mx::as_strided(audio, {1, features_length, static_cast<int>(c.win_length)},
                               {static_cast<int64_t>(count), static_cast<int64_t>(c.hop_length), 1}, 0);
  auto spectrum = mx::fft::rfft(mx::multiply(frames, *m.window), static_cast<int>(c.n_fft), -1);
  auto power = mx::square(mx::abs(spectrum));
  auto mel = mx::matmul(power, *m.mel_filter);
  auto features = mx::log(mx::clip(mel, mx::array(1e-9f), mx::array(1e9f)));
  auto x = mx::astype(features, m.dtype);  // [1, F, 64]

  // -- striding subsampling, x4 --
  const int sk = static_cast<int>(c.subs_kernel_size);
  x = relu(conv(*m.subsample1, x, 2, (sk - 1) / 2, 1));
  x = relu(conv(*m.subsample2, x, 2, (sk - 1) / 2, 1));
  const int encoded_length = subsampled_length(subsampled_length(features_length, sk), sk);
  const int t = x.shape(1);
  // ASSUMPTION: at batch 1 the conv output length equals encoded_length, so
  // the reference's _time_mask calls are no-ops.

  // -- rotary tables, exactly as ConformerEncoder._rotary --
  // NOTE: the reference casts the angles to fp16 BEFORE cos/sin; upstream
  // PyTorch takes cos/sin in float32. Parity with gigaam-stt needs the fp16
  // cast; measure whether float32 tables change WER (docs, open question).
  const int head_dim = m.head_dim();
  auto exponents = mx::divide(mx::arange(0.0, static_cast<double>(head_dim), 2.0, mx::float32),
                              mx::array(static_cast<float>(head_dim)));
  auto inv_freq = mx::reciprocal(mx::power(mx::array(static_cast<float>(c.pos_emb_base)), exponents));
  auto positions = mx::arange(0.0, static_cast<double>(t), 1.0, mx::float32);
  auto freqs = mx::multiply(mx::reshape(positions, {t, 1}), mx::reshape(inv_freq, {1, head_dim / 2}));
  auto embedding = mx::astype(mx::concatenate({freqs, freqs}, -1), x.dtype());
  auto cos = mx::cos(embedding);
  auto sin = mx::sin(embedding);

  // -- 24 Conformer layers, with cancel points every 8 --
  const bool cancellable = options != nullptr && options->should_cancel != nullptr;
  for (size_t i = 0; i < m.layers.size(); ++i) {
    x = conformer_layer(m.layers[i], x, cos, sin, c, head_dim);
    if (cancellable && (i + 1) % 8 == 0 && i + 1 < m.layers.size()) {
      mx::eval(x);
      if (options->should_cancel(options->cancel_user)) fail(GIGAAM_MLX_CANCELED, "canceled");
    }
  }

  // -- CTC head: 1x1 conv, then log-softmax --
  auto logits = conv(*m.head, x, 1, 0, 1);
  auto log_probs = mx::subtract(logits, mx::logsumexp(logits, -1, /*keepdims=*/true));
  return Forward{log_probs, std::min(encoded_length, t)};
}

// --------------------------------------------------------------- decoding

struct Word {
  std::string text;
  double start;  // seconds, absolute
  double end;
};

struct Token {
  uint32_t id;
  uint32_t frame;
  double time;  // seconds, absolute
};

struct ChunkDecode {
  std::string text;
  std::vector<Token> tokens;
  std::vector<Word> words;  // chunk-relative times until shifted
};

std::string trim(const std::string& text) {
  auto first = text.find_first_not_of(" \t\n\r\f\v");
  if (first == std::string::npos) return {};
  auto last = text.find_last_not_of(" \t\n\r\f\v");
  return text.substr(first, last - first + 1);
}

// GigaAMCTC.greedy_decode plus service._word_timestamps.
ChunkDecode decode(const Model& m, const Forward& f, size_t chunk_samples) {
  auto labels = mx::argmax(f.log_probs, -1);  // [1, T] uint32
  mx::eval(labels);
  const auto* ids = labels.data<uint32_t>();
  ChunkDecode out;
  const int blank = m.blank();
  int64_t previous = -1;
  for (int frame = 0; frame < f.encoded_length; ++frame) {
    const int64_t token = ids[frame];
    if (token != blank && token != previous) {
      out.tokens.push_back(Token{static_cast<uint32_t>(token), static_cast<uint32_t>(frame), 0.0});
      out.text += m.vocabulary[static_cast<size_t>(token)];
    }
    previous = token;
  }
  if (f.encoded_length <= 0) return out;
  const double shift = static_cast<double>(chunk_samples) / m.config.sample_rate / f.encoded_length;
  std::string chars;
  std::vector<uint32_t> frames;
  auto commit = [&] {
    auto text = trim(chars);
    if (!text.empty() && !frames.empty()) {
      out.words.push_back(Word{text, frames.front() * shift, (frames.back() + 1) * shift});
    }
    chars.clear();
    frames.clear();
  };
  for (auto& token : out.tokens) {
    token.time = token.frame * shift;
    const auto& piece = m.vocabulary[token.id];
    if (piece == " ") {
      commit();
    } else {
      chars += piece;
      frames.push_back(token.frame);
    }
  }
  commit();
  return out;
}

}  // namespace

// ------------------------------------------------------------------- C ABI

struct gigaam_mlx_session {
  std::unique_ptr<Model> model;
};

struct gigaam_mlx_result {
  std::string text;
  std::vector<std::string> word_text;
  std::vector<gigaam_mlx_word> words;  // text pointers into word_text
  std::vector<gigaam_mlx_token> tokens;
};

namespace {

gigaam_mlx_result* transcribe(const Model& m, const float* samples, size_t count,
                              const gigaam_mlx_transcribe_options* options) {
  const double rate = m.config.sample_rate;
  // chunk_seconds 0 selects both defaults; otherwise overlap is taken as given.
  const bool custom = options != nullptr && options->chunk_seconds > 0;
  const float chunk_seconds = custom ? options->chunk_seconds : 20.0f;
  const float overlap_seconds = custom ? options->overlap_seconds : 2.0f;
  if (overlap_seconds < 0 || overlap_seconds >= chunk_seconds) {
    fail(GIGAAM_MLX_INVALID, "require chunk_seconds > overlap_seconds >= 0");
  }
  // audio.fixed_chunks
  const size_t size = static_cast<size_t>(std::llround(chunk_seconds * rate));
  const size_t overlap = static_cast<size_t>(std::llround(overlap_seconds * rate));
  const size_t step = size - overlap;
  std::vector<std::pair<size_t, size_t>> ranges;
  for (size_t start = 0; start < count; start += step) {
    const size_t end = std::min(start + size, count);
    ranges.emplace_back(start, end);
    if (end == count) break;
  }

  auto result = std::make_unique<gigaam_mlx_result>();
  std::vector<Word> kept;
  std::vector<std::string> chunk_texts;
  for (size_t index = 0; index < ranges.size(); ++index) {
    if (options && options->should_cancel && options->should_cancel(options->cancel_user)) {
      fail(GIGAAM_MLX_CANCELED, "canceled");
    }
    const auto [start, end] = ranges[index];
    auto f = forward(m, samples + start, end - start, options);
    ChunkDecode decoded = f ? decode(m, *f, end - start) : ChunkDecode{};
    chunk_texts.push_back(decoded.text);
    // service.transcribe_file's keep window.
    const double start_s = start / rate;
    const double end_s = end / rate;
    const bool last = index + 1 == ranges.size();
    const double keep_start = index == 0 ? start_s : start_s + overlap_seconds / 2.0;
    const double keep_end = last ? end_s : end_s - overlap_seconds / 2.0;
    auto keeps = [&](double midpoint) {
      return (keep_start <= midpoint && midpoint < keep_end) || (last && midpoint <= keep_end);
    };
    for (const auto& word : decoded.words) {
      Word global{word.text, start_s + word.start, start_s + word.end};
      if (keeps((global.start + global.end) / 2.0)) kept.push_back(global);
    }
    // Hex extra: the tokens inside the same keep window, for per-token times.
    for (const auto& token : decoded.tokens) {
      const double time = start_s + token.time;
      if (keeps(time)) {
        result->tokens.push_back(gigaam_mlx_token{token.id, token.frame, std::llround(time * 1000.0)});
      }
    }
  }

  // service._monotonic_words
  const double duration = count / rate;
  double previous_end = 0.0;
  for (auto& word : kept) {
    word.start = std::min(duration, std::max(previous_end, word.start));
    word.end = std::min(duration, std::max(word.start, word.end));
    previous_end = word.end;
  }
  if (!kept.empty()) {
    for (size_t i = 0; i < kept.size(); ++i) {
      if (i) result->text += " ";
      result->text += kept[i].text;
    }
  } else {
    std::string joined;
    for (size_t i = 0; i < chunk_texts.size(); ++i) {
      if (i) joined += " ";
      joined += trim(chunk_texts[i]);
    }
    result->text = trim(joined);
  }
  result->word_text.reserve(kept.size());
  for (const auto& word : kept) result->word_text.push_back(word.text);
  for (size_t i = 0; i < kept.size(); ++i) {
    result->words.push_back(gigaam_mlx_word{result->word_text[i].c_str(),
                                            std::llround(kept[i].start * 1000.0),
                                            std::llround(kept[i].end * 1000.0)});
  }
  if (options && options->release_cache) mx::clear_cache();
  return result.release();
}

}  // namespace

extern "C" {

const char* gigaam_mlx_version(void) {
  // ASSUMPTION: mx::version() exists in v0.32.4 (mlx/version.h); else drop it.
  static const std::string version = "gigaam_mlx " + std::to_string(GIGAAM_MLX_ABI_VERSION) +
                                     " (mlx " + mx::version() + ")";
  return version.c_str();
}

int32_t gigaam_mlx_metal_available(void) {
  std::lock_guard<std::mutex> lock(mlx_lock());
  return mx::metal::is_available() ? 1 : 0;
}

gigaam_mlx_session* gigaam_mlx_load(const gigaam_mlx_config* config,
                                    const gigaam_mlx_load_options* options,
                                    gigaam_mlx_status* status, char** error) {
  return guarded(status, error, [&]() -> gigaam_mlx_session* {
    if (config == nullptr || options == nullptr) fail(GIGAAM_MLX_INVALID, "config and options are required");
    std::lock_guard<std::mutex> lock(mlx_lock());
    if (options->metallib_path != nullptr) {
      // Must precede the first MLX operation in the process. A second load
      // with the same path is harmless; a different path is ignored by MLX.
      mx::metal::set_metallib_path(options->metallib_path);
    }
    if (!mx::metal::is_available()) fail(GIGAAM_MLX_NO_METAL, "MLX found no Metal device");
    // Strict Metal: never run on the CPU device.
    mx::set_default_device(mx::Device::gpu);
    if (options->cache_limit_bytes > 0) mx::set_cache_limit(options->cache_limit_bytes);
    auto session = std::make_unique<gigaam_mlx_session>();
    session->model = load_model(*config, *options);
    if (options->prewarm) {
      std::vector<float> silence(config->sample_rate, 0.0f);
      auto f = forward(*session->model, silence.data(), silence.size(), nullptr);
      if (f) mx::eval(f->log_probs);
    }
    return session.release();
  });
}

gigaam_mlx_result* gigaam_mlx_transcribe(gigaam_mlx_session* session, const float* samples,
                                         size_t sample_count,
                                         const gigaam_mlx_transcribe_options* options,
                                         gigaam_mlx_status* status, char** error) {
  return guarded(status, error, [&]() -> gigaam_mlx_result* {
    if (session == nullptr || (samples == nullptr && sample_count > 0)) {
      fail(GIGAAM_MLX_INVALID, "session and samples are required");
    }
    std::lock_guard<std::mutex> lock(mlx_lock());
    return transcribe(*session->model, samples, sample_count, options);
  });
}

gigaam_mlx_status gigaam_mlx_log_probs(gigaam_mlx_session* session, const float* samples,
                                       size_t sample_count, float* out, size_t out_capacity,
                                       size_t* frames, char** error) {
  gigaam_mlx_status status = GIGAAM_MLX_OK;
  guarded(&status, error, [&]() -> int {
    if (session == nullptr || frames == nullptr) fail(GIGAAM_MLX_INVALID, "session and frames are required");
    std::lock_guard<std::mutex> lock(mlx_lock());
    const auto& m = *session->model;
    auto f = forward(m, samples, sample_count, nullptr);
    *frames = f ? static_cast<size_t>(f->encoded_length) : 0;
    if (!f || out == nullptr) return 0;
    const size_t classes = m.config.num_classes;
    if (out_capacity < *frames * classes) fail(GIGAAM_MLX_INVALID, "out is too small");
    auto values = mx::astype(f->log_probs, mx::float32);
    mx::eval(values);
    std::memcpy(out, values.data<float>(), *frames * classes * sizeof(float));
    return 0;
  });
  return status;
}

const char* gigaam_mlx_result_text(const gigaam_mlx_result* result) {
  return result ? result->text.c_str() : "";
}

size_t gigaam_mlx_result_word_count(const gigaam_mlx_result* result) {
  return result ? result->words.size() : 0;
}

gigaam_mlx_word gigaam_mlx_result_word(const gigaam_mlx_result* result, size_t index) {
  if (!result || index >= result->words.size()) return gigaam_mlx_word{"", 0, 0};
  return result->words[index];
}

size_t gigaam_mlx_result_token_count(const gigaam_mlx_result* result) {
  return result ? result->tokens.size() : 0;
}

gigaam_mlx_token gigaam_mlx_result_token(const gigaam_mlx_result* result, size_t index) {
  if (!result || index >= result->tokens.size()) return gigaam_mlx_token{0, 0, 0};
  return result->tokens[index];
}

void gigaam_mlx_result_free(gigaam_mlx_result* result) { delete result; }

void gigaam_mlx_memory(uint64_t* active, uint64_t* peak, uint64_t* cache) {
  std::lock_guard<std::mutex> lock(mlx_lock());
  if (active) *active = mx::get_active_memory();
  if (peak) *peak = mx::get_peak_memory();
  if (cache) *cache = mx::get_cache_memory();  // ASSUMPTION: in memory.h beside the others
}

void gigaam_mlx_reset_peak_memory(void) {
  std::lock_guard<std::mutex> lock(mlx_lock());
  mx::reset_peak_memory();
}

void gigaam_mlx_free(gigaam_mlx_session* session) {
  if (session == nullptr) return;
  std::lock_guard<std::mutex> lock(mlx_lock());
  delete session;
  mx::clear_cache();
}

void gigaam_mlx_string_free(char* string) { std::free(string); }

}  // extern "C"
