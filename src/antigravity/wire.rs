//! Text-only first-turn wire format from pi-antigravity.
use super::*;
use sha2::{Digest, Sha256};
static MODEL_ENUMS: Mutex<Option<std::collections::HashMap<String, String>>> = Mutex::new(None);
pub(super) fn usable_model(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && !["model_", "chat_", "tab_"]
            .iter()
            .any(|p| id.to_ascii_lowercase().starts_with(p))
        && !id.to_ascii_lowercase().contains("image")
}
pub(super) fn remember_models(models: &serde_json::Map<String, Value>) {
    *MODEL_ENUMS.lock().unwrap_or_else(|e| e.into_inner()) = Some(
        models
            .iter()
            .take(256)
            .filter_map(|(id, v)| {
                v["model"]
                    .as_str()
                    .map(|label| (id.clone(), label.to_owned()))
            })
            .collect(),
    );
}

pub(super) fn random() -> Result<[u8; 32]> {
    let mut b = [0; 32];
    getrandom::fill(&mut b).map_err(|_| eyre!("Secure randomness unavailable"))?;
    Ok(b)
}
pub(super) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..c.len() + 1 {
            out.push(ALPHABET[((n >> (18 - i * 6)) & 63) as usize] as char);
        }
    }
    out
}
fn uuid() -> Result<String> {
    let mut b = random()?;
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    let h: String = b[..16].iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}
pub(super) fn challenge(verifier: &str) -> String {
    base64(&Sha256::digest(verifier.as_bytes()))
}
pub(super) fn envelope(
    project: &str,
    model: &str,
    instructions: &str,
    prompt: &str,
) -> Result<Value> {
    // Public picker IDs use the reference package's reasoning-off runtime routes.
    let model = match model {
        "gemini-3.8-flash" => "gemini-3.8-flash-low",
        "gemini-3.7-flash" => "gemini-3.7-flash-low",
        "gemini-3.6-flash" => "gemini-3.6-flash-low",
        "gemini-3.5-flash" => "gemini-3.5-flash-extra-low",
        "gemini-3.1-pro" => "gemini-3.1-pro-low",
        "claude-opus-4-6" => "claude-opus-4-6-thinking",
        "gpt-oss-120b" => "gpt-oss-120b-medium",
        other => other,
    };
    let agent = uuid()?;
    let trajectory = uuid()?;
    let b = random()?;
    let session = (u64::from_le_bytes(b[..8].try_into().unwrap()) & i64::MAX as u64).to_string();
    let claude = model.starts_with("claude-");
    let mut body = json!({
        "project": project,
        "model": model,
        "request": {
            "contents": [{"role": "user", "parts": [{"text": prompt}]}],
            "systemInstruction": {"role": "user", "parts": [{"text": instructions}]},
            "generationConfig": {
                "maxOutputTokens": 8192,
                "thinkingConfig": {"includeThoughts": false, "thinkingBudget": 0}
            },
            "sessionId": session,
            "labels": {
                "last_step_index": "0",
                "request_id": format!("{trajectory}-0"),
                "trajectory_id": trajectory,
                "used_claude": claude.to_string(),
                "used_claude_conservative": claude.to_string(),
                "used_non_gemini_model": (!model.starts_with("gemini-")).to_string()
            }
        },
        "requestType": "agent",
        "userAgent": "antigravity",
        "requestId": format!("agent/{agent}/{}/{trajectory}/1", now())
    });
    // The reference's ANTIGRAVITY_MODEL_ENUM, so a catalog pick still carries
    // its label after a restart, before the catalog is fetched again.
    let model_enum = match model {
        "gemini-3.8-flash" | "gemini-3.8-flash-high" => Some("MODEL_PLACEHOLDER_M318"),
        "gemini-3.8-flash-medium" => Some("MODEL_PLACEHOLDER_M319"),
        "gemini-3.8-flash-low" => Some("MODEL_PLACEHOLDER_M320"),
        "gemini-3.8-flash-tiered" => Some("MODEL_PLACEHOLDER_M322"),
        "gemini-3.7-flash" | "gemini-3.7-flash-high" => Some("MODEL_PLACEHOLDER_M298"),
        "gemini-3.7-flash-medium" => Some("MODEL_PLACEHOLDER_M299"),
        "gemini-3.7-flash-low" => Some("MODEL_PLACEHOLDER_M300"),
        "gemini-3.7-flash-tiered" => Some("MODEL_PLACEHOLDER_M301"),
        "gemini-3.6-flash" | "gemini-3.6-flash-high" => Some("MODEL_PLACEHOLDER_M71"),
        "gemini-3.6-flash-medium" => Some("MODEL_PLACEHOLDER_M72"),
        "gemini-3.6-flash-low" => Some("MODEL_PLACEHOLDER_M73"),
        "gemini-3.6-flash-tiered" => Some("MODEL_PLACEHOLDER_M196"),
        "gemini-3.5-flash" | "gemini-3.5-flash-low" => Some("MODEL_PLACEHOLDER_M20"),
        "gemini-3.5-flash-extra-low" => Some("MODEL_PLACEHOLDER_M187"),
        "gemini-3-flash-agent" => Some("MODEL_PLACEHOLDER_M84"),
        "gemini-3.1-pro" | "gemini-3.1-pro-low" => Some("MODEL_PLACEHOLDER_M36"),
        "gemini-3.1-pro-high" => Some("MODEL_PLACEHOLDER_M37"),
        "gemini-pro-agent" => Some("MODEL_PLACEHOLDER_M16"),
        "claude-sonnet-4-6" => Some("MODEL_PLACEHOLDER_M35"),
        "claude-opus-4-6" | "claude-opus-4-6-thinking" => Some("MODEL_PLACEHOLDER_M26"),
        "gpt-oss-120b" | "gpt-oss-120b-medium" => Some("MODEL_OPENAI_GPT_OSS_120B_MEDIUM"),
        _ => None,
    };
    let discovered = MODEL_ENUMS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(model))
        .cloned();
    if let Some(value) = discovered.as_deref().or(model_enum) {
        body["request"]["labels"]["model_enum"] = value.into();
    }
    if model.starts_with("gemini-4")
        || model.contains("argon")
        || !["gemini-", "claude-", "gpt-oss-"]
            .iter()
            .any(|p| model.starts_with(p))
    {
        body["request"]["generationConfig"]
            .as_object_mut()
            .unwrap()
            .remove("thinkingConfig");
    }
    Ok(body)
}
pub(super) fn parse_sse(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes).map_err(|_| eyre!("Invalid Antigravity stream"))?;
    let text = text.replace("\r\n", "\n");
    let mut answer = String::new();
    let mut finished = false;
    for event in text.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data == "[DONE]" {
            finished = true;
            break;
        }
        if data.is_empty() {
            continue;
        }
        let v: Value =
            serde_json::from_str(&data).map_err(|_| eyre!("Invalid Antigravity stream event"))?;
        if v.get("error").is_some() {
            return Err(stream_error(&v));
        }
        let v = v.get("response").unwrap_or(&v);
        if v.get("error").is_some() || v["promptFeedback"].get("blockReason").is_some() {
            return Err(stream_error(v));
        }
        if let Some(candidates) = v["candidates"].as_array() {
            for candidate in candidates.iter().take(1) {
                if candidate["finishReason"] == "MAX_TOKENS" {
                    bail!("Antigravity stopped at its length limit");
                }
                if let Some(reason) = candidate["finishReason"].as_str()
                    && !["STOP", "FINISH_REASON_UNSPECIFIED"].contains(&reason)
                {
                    bail!("Antigravity generation did not finish");
                }
                finished |= candidate["finishReason"] == "STOP";
                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for part in parts {
                        if part["thought"] != true
                            && let Some(t) = part["text"].as_str()
                        {
                            answer.push_str(t);
                        }
                    }
                }
            }
        }
    }
    if !finished {
        bail!("Antigravity generation did not finish");
    }
    if answer.trim().is_empty() {
        bail!("Antigravity returned no text");
    }
    Ok(answer)
}
fn stream_error(value: &Value) -> color_eyre::eyre::Report {
    match value["error"]["code"].as_u64() {
        Some(code @ (401 | 429)) => Status(code as u16).into(),
        _ => eyre!("Antigravity generation failed"),
    }
}
// SHA-1 UUID-shaped fallback, matching stableProjectId(email) in the reference.
pub(super) fn stable_project(seed: &str) -> String {
    let mut bytes = format!("antigravity:{seed}").into_bytes();
    let bits = bytes.len() as u64 * 8;
    bytes.push(128);
    while bytes.len() % 64 != 56 {
        bytes.push(0);
    }
    bytes.extend(bits.to_be_bytes());
    let mut h = [
        0x67452301u32,
        0xefcdab89,
        0x98badcfe,
        0x10325476,
        0xc3d2e1f0,
    ];
    for chunk in bytes.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, b) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*b);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut b: Vec<u8> = h.iter().flat_map(|v| v.to_be_bytes()).take(16).collect();
    b[6] = (b[6] & 15) | 80;
    b[8] = (b[8] & 63) | 128;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
