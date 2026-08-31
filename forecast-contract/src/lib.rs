//! forecast-contract v0.1.1 — T3N Forecast Brief Agent (fase 2: LLM real en TEE).
//!
//! Contrato Rust->WASM component que corre en la TEE de T3N. Expone la función
//! `forecast` vía el interface WIT `contracts`. Recibe una pregunta de
//! forecasting + contexto y produce un Forecast Brief JSON:
//!   { summary, probability_estimate, reasoning, sources_placeholder, meta }
//!
//! FASE 2 (implementada): lee `llm_api_key` del KV map `z:<tid>:secrets` y
//! llama a OpenRouter (chat/completions) vía `host:interfaces/http`. El reply
//! del LLM se parsea a { summary, probability_estimate, reasoning, sources }.
//! Fallback informativo sin panic si la llamada falla (e.g. egress_denied,
//! red, HTTP != 200, JSON malformado).
//!
//! # Host-capability manifest (para el registro)
//! ```json
//! { "host_capabilities": ["kv_store", "logging", "tenant_context", "http"] }
//! ```

extern crate alloc;

pub const CONTRACT_VERSION: &str = "0.1.1";

wit_bindgen::generate!({
    world: "forecast-contract",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

mod forecast;

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::forecast_contract::contracts::Guest for Component {
    fn forecast(
        req: exports::z::forecast_contract::contracts::GenericInput,
    ) -> Result<Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("forecast: missing input")?;
        forecast::forecast(&input)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;
    use crate::forecast::{forecast, ForecastInput};

    #[test]
    fn contract_version_is_semver() {
        let parts: Vec<&str> = CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "CONTRACT_VERSION must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "each part must be a number");
        }
    }

    #[test]
    fn forecast_produces_brief_shape() {
        let input = ForecastInput {
            question: "Will the Fed cut rates before 2026-12-31?".into(),
            context: "FOMC minutes hint at one cut; CPI cooling for 3 months.".into(),
            llm_key_placeholder: None,
            model: None,
        };
        let bytes = serde_json::to_vec(&input).unwrap();
        let out = forecast(&bytes).expect("forecast should succeed on valid input");
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("summary").is_some(), "brief must have summary");
        assert!(v.get("probability_estimate").is_some(), "brief must have probability_estimate");
        assert!(v.get("reasoning").is_some(), "brief must have reasoning");
        assert!(v.get("sources_placeholder").is_some(), "brief must have sources_placeholder");
        assert!(v.get("meta").is_some(), "brief must have meta");
    }

    #[test]
    fn forecast_rejects_empty_question() {
        let input = ForecastInput {
            question: "".into(),
            context: "x".into(),
            llm_key_placeholder: None,
            model: None,
        };
        let bytes = serde_json::to_vec(&input).unwrap();
        let out = forecast(&bytes);
        assert!(out.is_err(), "empty question must be rejected");
    }
}