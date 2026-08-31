//! Lógica de forecasting — fase 2: LLM real (OpenRouter) vía host:interfaces/http.
//!
//! Nativo (tests): placeholder determinístico de fase 1. WASM (TEE): lee
//! `llm_api_key` de `z:<tid>:secrets` (kv-store), POST a OpenRouter
//! chat/completions (host:interfaces/http), parsea el reply del LLM y extrae
//! { summary, probability_estimate, reasoning, sources }. Si algo falla
//! (egress_denied, red, HTTP != 200, JSON malformado) → brief fallback
//! informativo con el error crudo — nunca panic.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Input JSON del contract.
/// `llm_key_placeholder` se conserva por compatibilidad de fase 1 (solo meta).
/// `model` (fase 2): modelo OpenRouter a usar, p.ej. "openai/gpt-4o-mini" o
/// "openrouter/free". None → default compilado (LLM_MODEL). Permite probar
/// modelos gratis sin recompilar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastInput {
    pub question: String,
    #[serde(default)]
    pub context: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_key_placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Output JSON del contract — el Forecast Brief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastBrief {
    pub summary: String,
    /// Probabilidad estimada en [0.0, 1.0] — parseada del reply del LLM.
    pub probability_estimate: f64,
    pub reasoning: String,
    /// Fuentes citadas por el LLM.
    pub sources_placeholder: Vec<String>,
    pub meta: BriefMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefMeta {
    pub contract_version: String,
    pub phase: String,
    pub placeholder: bool,
    pub input_chars: usize,
    pub deterministic_seed: u64,
    /// Fase 2: trazabilidad — modelo usado y origen de la probabilidad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probability_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_error: Option<String>,
}

/// Punto de entrada de la lógica: bytes JSON in -> bytes JSON out.
pub fn forecast(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: ForecastInput = serde_json::from_slice(input)
        .map_err(|e| format!("forecast: invalid input JSON: {e}"))?;

    if req.question.trim().is_empty() {
        return Err("forecast: `question` must be a non-empty string".into());
    }

    #[cfg(target_arch = "wasm32")]
    {
        let brief = match llm_brief(&req) {
            Ok(b) => b,
            Err(e) => {
                let _ = logging::info(&format!(
                    "[forecast] LLM path failed, entregando brief fallback: {e}"
                ));
                fallback_brief(&req, &e)
            }
        };
        serde_json::to_vec(&brief).map_err(|e| format!("forecast: serialize brief: {e}"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let brief = placeholder_brief(&req);
        serde_json::to_vec(&brief).map_err(|e| format!("forecast: serialize brief: {e}"))
    }
}

/// Placeholder determinístico (se mantiene para tests nativos y como
/// documentación del shape). La "estimación" es FNV-1a sobre el texto.
fn placeholder_brief(req: &ForecastInput) -> ForecastBrief {
    let seed = fnv1a(req.question.as_bytes());
    let probability = 0.05 + (seed % 1000) as f64 / 1000.0 * 0.90;

    ForecastBrief {
        summary: format!(
            "[PLACEHOLDER] Preliminary brief for: {}",
            truncate(&req.question, 120)
        ),
        probability_estimate: (probability * 1000.0).round() / 1000.0,
        reasoning: format!(
            "[PLACEHOLDER — no LLM call in native tests] Inputs received: \
             question ({} chars) + context ({} chars).",
            req.question.chars().count(),
            req.context.chars().count()
        ),
        sources_placeholder: Vec::new(),
        meta: BriefMeta {
            contract_version: super::CONTRACT_VERSION.to_string(),
            phase: "1-skeleton".to_string(),
            placeholder: true,
            input_chars: req.question.chars().count() + req.context.chars().count(),
            deterministic_seed: seed,
            model: None,
            probability_source: Some("fnv-placeholder".to_string()),
            llm_error: None,
        },
    }
}

/// FNV-1a (64-bit) — determinístico y sin dependencias.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

// ============================================================================
// Fase 2: LLM real dentro del TEE (solo wasm32 — los imports de host bindgen
// existen solo para el target wasm; nativo usa el placeholder de fase 1).
// ============================================================================

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http as http_iface, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Default barato para probar el pipeline; sobreescribible vía input.model.
const LLM_MODEL: &str = "openai/gpt-4o-mini";

const SYSTEM_PROMPT: &str = "You are Superforecaster, an expert judgmental forecaster. Given a forecasting question and context, produce a calibrated forecast. Respond with ONLY a JSON object, no markdown, no prose outside it, with exactly these keys: \"summary\" (1-2 sentences), \"probability_estimate\" (number in [0,1]), \"reasoning\" (3-6 sentences: base rates, evidence, key uncertainties), \"sources\" (array of source names or URLs you rely on; empty array if none).";

/// Lee la OpenRouter key del map `z:<tid>:secrets` (key: llm_api_key).
/// Mismo patrón que z-tenant-flight get_api_key().
#[cfg(target_arch = "wasm32")]
fn read_llm_key() -> Result<String, String> {
    let tid = tenant_context::tenant_did();
    let map_name = format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, b"llm_api_key")
        .map_err(|e| format!("kv read: {e}"))?
        .ok_or(
            "llm_api_key not found in z:<tid>:secrets — seed it via map-entry-set",
        )?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Headers OpenRouter. Content-Type lo setea el host HTTP (mismo patrón que
/// z-tenant-flight con Duffel: mandarlo explícito crea header duplicado).
#[cfg(target_arch = "wasm32")]
fn openrouter_headers(
    api_key: &str,
) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_string(), format!("Bearer {api_key}")),
        ("Accept".to_string(), "application/json".to_string()),
        (
            "HTTP-Referer".to_string(),
            "https://t3n.forecast-brief-agent".to_string(),
        ),
        ("X-Title".to_string(), "forecast-contract".to_string()),
    ]
}

/// Path LLM real: key -> request -> parse. Cualquier error sube como Result
/// y `forecast()` lo convierte en brief fallback (nunca panic).
#[cfg(target_arch = "wasm32")]
fn llm_brief(req: &ForecastInput) -> Result<ForecastBrief, String> {
    let api_key = read_llm_key()?;
    let model = req.model.clone().unwrap_or_else(|| LLM_MODEL.to_string());
    let user_prompt = format!(
        "Question: {}\n\nContext: {}\n\nProduce the JSON forecast brief now.",
        req.question, req.context
    );
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ],
        "temperature": 0.2,
        "max_tokens": 900,
    });

    let _ = logging::info(&format!("[forecast] OpenRouter call: model={model}"));

    let resp = http_iface::call(&http_iface::Request {
        method: http_iface::Verb::Post,
        url: String::from(OPENROUTER_URL),
        headers: Some(openrouter_headers(&api_key)),
        payload: Some(
            serde_json::to_vec(&body).map_err(|e| format!("serialize body: {e}"))?,
        ),
    })
    .map_err(|e| format!("http outbound call failed: {e}"))?;

    if resp.code != 200 {
        let txt = String::from_utf8_lossy(&resp.payload);
        let short: String = txt.chars().take(400).collect();
        return Err(format!("openrouter HTTP {}: {}", resp.code, short));
    }

    let json: serde_json::Value = serde_json::from_slice(&resp.payload)
        .map_err(|e| format!("openrouter response parse: {e}"))?;
    let used_model = json["model"].as_str().unwrap_or(&model).to_string();
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("openrouter: no choices[0].message.content")?
        .to_string();

    let parsed = parse_brief(&content)?;
    let _ = logging::info(&format!(
        "[forecast] brief parsed: p={:.3} source={} sources={}",
        parsed.probability,
        parsed.probability_source,
        parsed.sources.len()
    ));

    Ok(ForecastBrief {
        summary: parsed.summary,
        probability_estimate: parsed.probability,
        reasoning: parsed.reasoning,
        sources_placeholder: parsed.sources,
        meta: BriefMeta {
            contract_version: super::CONTRACT_VERSION.to_string(),
            phase: "2-llm-in-tee".to_string(),
            placeholder: false,
            input_chars: req.question.chars().count() + req.context.chars().count(),
            deterministic_seed: fnv1a(req.question.as_bytes()),
            model: Some(used_model),
            probability_source: Some(parsed.probability_source.to_string()),
            llm_error: None,
        },
    })
}

/// Brief fallback informativo si la llamada LLM falla — nunca panic.
#[cfg(target_arch = "wasm32")]
fn fallback_brief(req: &ForecastInput, err: &str) -> ForecastBrief {
    ForecastBrief {
        summary: format!(
            "[FALLBACK] the in-TEE LLM call failed: {}",
            truncate(err, 160)
        ),
        probability_estimate: 0.5,
        reasoning: format!(
            "Fallback brief: the phase-2 LLM call inside the TEE did not complete, \
             so no calibrated estimate is available. Raw error: {err}"
        ),
        sources_placeholder: Vec::new(),
        meta: BriefMeta {
            contract_version: super::CONTRACT_VERSION.to_string(),
            phase: "2-llm-fallback".to_string(),
            placeholder: true,
            input_chars: req.question.chars().count() + req.context.chars().count(),
            deterministic_seed: fnv1a(req.question.as_bytes()),
            model: Some(LLM_MODEL.to_string()),
            probability_source: Some("default-0.5".to_string()),
            llm_error: Some(truncate(err, 600)),
        },
    }
}

struct ParsedBrief {
    summary: String,
    probability: f64,
    probability_source: &'static str,
    reasoning: String,
    sources: Vec<String>,
}

/// Parsea el texto del LLM a un brief estructurado. Pure — testeable nativo.
fn parse_brief(content: &str) -> Result<ParsedBrief, String> {
    let cleaned = strip_code_fences(content);
    let v = try_parse_json_object(cleaned)
        .ok_or_else(|| "LLM reply contains no JSON object".to_string())?;

    let summary = v["summary"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate(s, 280))
        .unwrap_or_else(|| truncate(cleaned, 200));

    let (probability, probability_source): (f64, &'static str) =
        if let Some(p) = v["probability_estimate"].as_f64() {
            (normalize_prob(p), "llm-json")
        } else if let Some(p) = v["probability_estimate"].as_str().and_then(parse_percent_str) {
            (normalize_prob(p), "llm-json-str")
        } else if let Some(p) = extract_probability_from_text(content) {
            (p, "text-scan")
        } else {
            (0.5, "default-0.5")
        };

    let reasoning = v["reasoning"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate(s, 2500))
        .unwrap_or_else(|| truncate(cleaned, 1500));

    let mut sources: Vec<String> = Vec::new();
    for key in ["sources", "citations", "references"] {
        if let Some(arr) = v[key].as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    if !s.trim().is_empty() {
                        sources.push(truncate(s.trim(), 200));
                    }
                }
            }
        }
        if !sources.is_empty() {
            break;
        }
    }

    Ok(ParsedBrief {
        summary,
        probability,
        probability_source,
        reasoning,
        sources,
    })
}

fn strip_code_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric());
        let rest = rest.trim_start();
        let rest = rest.strip_suffix("```").unwrap_or(rest);
        return rest.trim_end();
    }
    t
}

fn try_parse_json_object(s: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        if v.is_object() {
            return Some(v);
        }
    }
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&s[start..=end]).ok()
}

fn normalize_prob(p: f64) -> f64 {
    // Acepta 0-1 y también 0-100 (los LLMs a veces devuelven porcentaje).
    let p = if p > 1.0 && p <= 100.0 { p / 100.0 } else { p };
    p.clamp(0.0, 1.0)
}

fn parse_percent_str(s: &str) -> Option<f64> {
    let t = s.trim().trim_end_matches('%').trim();
    t.parse::<f64>().ok()
}

/// Último recurso: escanea "NN%" cerca de palabras de probabilidad en el texto.
/// Evita leer tasas ("4.25%") como probabilidades exigiendo contexto cercano.
fn extract_probability_from_text(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'%' {
            continue;
        }
        let mut start = i;
        while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
            start -= 1;
        }
        if start == i || i - start > 6 {
            continue;
        }
        let num = &text[start..i];
        let v: f64 = match num.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let lo = i.saturating_sub(80);
        let hi = lower.len().min(i + 80);
        let window = &lower[lo..hi];
        let near_prob = ["probab", "chance", "likelihood", "odds"]
            .iter()
            .any(|w| window.contains(w));
        if !near_prob {
            continue;
        }
        if (0.0..=100.0).contains(&v) {
            return Some(normalize_prob(v / 100.0));
        }
    }
    None
}

#[cfg(test)]
mod llm_parse_tests {
    use super::*;

    #[test]
    fn parse_fenced_json_brief() {
        let out = parse_brief(
            "Sure! Here it is:\n```json\n{\"summary\":\"Fed likely cuts\",\"probability_estimate\":0.65,\"reasoning\":\"Base rates.\",\"sources\":[\"fomc.gov\"]}\n```",
        )
        .expect("should parse fenced JSON");
        assert_eq!(out.summary, "Fed likely cuts");
        assert!((out.probability - 0.65).abs() < 1e-9);
        assert_eq!(out.probability_source, "llm-json");
        assert_eq!(out.sources, vec!["fomc.gov".to_string()]);
    }

    #[test]
    fn parse_bare_json_brief() {
        let out = parse_brief(
            "{\"summary\":\"Cut likely\",\"probability_estimate\":\"55%\",\"reasoning\":\"R\",\"sources\":[]}",
        )
        .expect("should parse bare JSON with string prob");
        assert!((out.probability - 0.55).abs() < 1e-9);
        assert_eq!(out.probability_source, "llm-json-str");
    }

    #[test]
    fn probability_scan_percent_with_context() {
        assert_eq!(
            extract_probability_from_text("There is a 65% chance of a cut"),
            Some(0.65)
        );
    }

    #[test]
    fn probability_scan_ignores_interest_rates() {
        assert_eq!(
            extract_probability_from_text("The fed funds rate is 4.25% today"),
            None
        );
    }

    #[test]
    fn normalize_handles_percent_numbers() {
        assert!((normalize_prob(65.0) - 0.65).abs() < 1e-9);
        assert!((normalize_prob(0.4) - 0.4).abs() < 1e-9);
        assert_eq!(normalize_prob(150.0), 1.0);
    }

    #[test]
    fn parse_rejects_no_json() {
        assert!(parse_brief("no json at all here").is_err());
    }
}