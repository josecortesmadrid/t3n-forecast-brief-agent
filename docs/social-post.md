# Social post (BONUS) — borrador, NO publicar sin pase de Jose

## Tweet principal (EN)

> Shipped: a forecasting agent where the LLM runs **inside the TEE** 🔒
>
> Rust→WASM contract on @terminal3io testnet: API key sealed in enclave KV → OpenRouter call through egress-gated http → structured Forecast Brief (calibrated probability + reasoning).
>
> Live run: "Will the Fed cut rates before Oct 2026?" → 0.45
>
> Found & patched 3 SDK v5.3 bugs along the way (RTMR1 allowlist, manifest validation, delegation writes) — all documented.
>
> Repo: https://github.com/josecortesmadrid/t3n-forecast-brief-agent
>
> #ConfidentialComputing #AI #Forecasting

*(≈340 chars — cabe en un post; para tweet puro de X recortar a la variante corta de abajo.)*

## Variante corta (tweet estricto, <280)

> Built a forecasting agent on @terminal3io where the LLM call itself runs inside the TEE: sealed API key in enclave KV, egress-gated OpenRouter call, structured Forecast Brief out. Live on testnet: Fed-cut question → p=0.45. Repo + bug write-ups 👇
> https://github.com/josecortesmadrid/t3n-forecast-brief-agent

## Reply-thread sugerido (opcional)

1. Arquitectura en un diagrama (copiar el ASCII del README o screenshot del brief).
2. Los 3 bugs del SDK v5.3.0 como mini-hilo (buen contenido para el criterio "Bug submission quality").
3. Screenshot del Google Doc final.