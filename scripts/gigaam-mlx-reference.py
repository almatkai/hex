#!/usr/bin/env python3
"""Reference outputs from gigaam-multilingual-mlx for Hex's MLX parity check.

Developer tool only; Hex never ships or runs Python. On an Apple Silicon Mac:

    uv run --python 3.13 --with gigaam-multilingual-mlx==0.2.0 \
        python scripts/gigaam-mlx-reference.py --variant int8 \
        --out /tmp/gigaam-ref tests/fixtures/gigaam/*.wav

For each WAV it writes <stem>.json (text, token ids and frames of the
single-pass decode, chunked words, timings, memory) and <stem>.logprobs.npy
(float32 [frames, 71], one pass over the whole clip, no chunking: compare with
`gigaam_mlx_log_probs`). Audio is read as 16 kHz mono 16-bit PCM WAV with the
standard library, not ffmpeg, so Hex and Python see identical samples.

See docs/plans/gigaam-mlx.md, "Verification".
"""

from __future__ import annotations

import argparse
import json
import time
import wave
from pathlib import Path

import mlx.core as mx
import numpy as np
from gigaam_multilingual_mlx import load_model
from gigaam_multilingual_mlx.service import _monotonic_words, _word_timestamps
from gigaam_multilingual_mlx.audio import SAMPLE_RATE, fixed_chunks


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path), "rb") as wav:
        if wav.getframerate() != SAMPLE_RATE or wav.getnchannels() != 1 or wav.getsampwidth() != 2:
            raise SystemExit(f"{path}: need 16 kHz mono 16-bit PCM")
        frames = wav.readframes(wav.getnframes())
    return np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0


def chunked(model, audio: np.ndarray, chunk: float, overlap: float) -> dict:
    """service.transcribe_file without ffmpeg (same keep rule)."""
    ranges = list(fixed_chunks(audio, chunk, overlap))
    kept, texts = [], []
    for index, (start, end, samples) in enumerate(ranges):
        logits, lengths = model(mx.array(samples)[None, :], mx.array([len(samples)]))
        decoded = model.greedy_decode(logits, lengths)[0]
        texts.append(decoded["text"])
        words = _word_timestamps(model, decoded, len(samples), int(np.asarray(lengths)[0]))
        start_s, end_s = start / SAMPLE_RATE, end / SAMPLE_RATE
        last = index == len(ranges) - 1
        keep_start = start_s if index == 0 else start_s + overlap / 2
        keep_end = end_s if last else end_s - overlap / 2
        for word in words:
            item = {"text": word["text"], "start": start_s + word["start"], "end": start_s + word["end"]}
            mid = (item["start"] + item["end"]) / 2
            if keep_start <= mid < keep_end or (last and mid <= keep_end):
                kept.append(item)
    kept = _monotonic_words(kept, len(audio) / SAMPLE_RATE)
    text = " ".join(w["text"] for w in kept) or " ".join(t.strip() for t in texts).strip()
    return {"text": text, "words": kept}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("wavs", nargs="+", type=Path)
    parser.add_argument("--variant", default="int8", choices=("fp16", "int8"))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--repeat", type=int, default=5, help="timed runs per file")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    started = time.perf_counter()
    model = load_model(variant=args.variant)
    mx.eval(model.parameters())
    load_seconds = time.perf_counter() - started

    for path in args.wavs:
        audio = read_wav(path)
        logits, lengths = model(mx.array(audio)[None, :], mx.array([len(audio)]))
        mx.eval(logits, lengths)
        single = model.greedy_decode(logits, lengths)[0]
        np.save(args.out / f"{path.stem}.logprobs.npy", np.asarray(logits.astype(mx.float32))[0])

        result = chunked(model, audio, 20.0, 2.0)
        mx.reset_peak_memory()
        timings = []
        for _ in range(args.repeat):
            begin = time.perf_counter()
            chunked(model, audio, 20.0, 2.0)
            timings.append(time.perf_counter() - begin)
        record = {
            "audio": path.name,
            "samples": len(audio),
            "variant": args.variant,
            "single_pass": {
                "text": single["text"],
                "token_ids": single["token_ids"],
                "token_frames": single["token_frames"],
                "encoded_length": int(np.asarray(lengths)[0]),
            },
            "chunked": result,
            "seconds": {"load": load_seconds, "median": float(np.median(timings)), "runs": timings},
            "memory": {"metal_peak_bytes": mx.get_peak_memory(), "metal_active_bytes": mx.get_active_memory()},
        }
        (args.out / f"{path.stem}.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
        print(f"{path.name}: {result['text']}")


if __name__ == "__main__":
    main()
