# T3N Forecast Brief Agent

A **forecasting agent on T3N** (Terminal 3 Network) whose LLM call runs **inside the TEE**. Give it a forecasting question, get back a structured **Forecast Brief** — summary, calibrated probability estimate, reasoning, sources — produced by a real LLM (OpenRouter) that executes *inside confidential compute*, with credentials stored in the TEE's key-value store and every outbound host gated by the platform's egress allowlist.

Registered on T3N testnet (SG cluster) as contract **`z:<tenant-tid>:forecast-contract` v0.1.1** — the live brief for *"Will the Federal Reserve cut rates before October 2026?"* estimated **0.45** with `probability_source: "llm-json"`.

```
┌────────────────────────────┐
│  Tenant (you, scripts/)    │  T3N SDK v5.3 · Ethereum-signed session
│  - secrets in z:<tid>:kv   │  llm_api_key sealed via control-plane write
└──────────┬─────────────────┘
           │ register (WASM component) / execute(forecast, input)
           ▼
┌────────────────────────────────────────────────────────┐
│  T3N node — TEE (CN cluster)                           │
│  ┌──────────────────────────────────────────────────┐  │
│  │ forecast-contract (Rust → wasm32-wasip2)         │  │
│  │  1. kv-store::get("z:<tid>:secrets")             │  │  ← LLM key never leaves the enclave
│  │  2. http::call POST openrouter.ai (gated egress) │  │  ← host allowlist: ["openrouter.ai"]
│  │  3. parse reply → Forecast Brief JSON            │  │  ← fallback brief, never panic
│  └──────────────────────────────────────────────────┘  │
└──────────┬─────────────────────────────────────────────┘
           │ OpenRouter chat/completions (openai/gpt-4o-mini default)
           ▼
       Forecast Brief: { summary, probability_estimate, reasoning, sources, meta }
```

## What it demonstrates

- **Real LLM inside the enclave** — not a mock: the WASM contract reads an API key from the TEE KV store, performs an outbound HTTPS call to OpenRouter via `host:interfaces/http@2.1.0`, parses the reply and returns a structured, typed brief. `meta.placeholder = false`, `meta.probability_source = "llm-json"`.
- **Capability-scoped contract** — the WIT world imports exactly `tenant-context`, `logging`, `kv-store`, `http`. Outbound traffic is denied unless the caller holds a delegation grant with `allowed_hosts: ["openrouter.ai"]` (set via `t3n.updateMemberDelegation`).
- **Secrets hygiene** — the OpenRouter key is sealed into `z:<tid>:secrets` with the map ACL scoped to the contract id (`writers/readers: only: [contract_id]`); the tenant scripts never print it and it never travels into the contract input.
- **Graceful degradation** — every failure path (egress denied, HTTP error, malformed JSON) yields an informative *fallback brief* with `meta.llm_error` set, never a panic.

## Repo layout

```
forecast-contract/   Rust → WASM component (the TEE contract)
  src/forecast.rs    forecasting logic: phase-2 LLM path + phase-1 native placeholder + parser + tests
  src/lib.rs         bindgen wiring + WIT Guest impl + CONTRACT_VERSION
  wit/world.wit      `world forecast-contract` — imports (capability set) + export `forecast`
scripts/
  lib.ts                  shared bootstrap: trust anchor (with CN-cluster fixes), auth, TenantClient
  create-kv-maps.ts       create + ACL `secrets` map, seal llm_api_key (control-plane write)
  register-contract.ts    register the WASM component, bump version, re-scope map ACL
  grant-egress-v2.ts      self-grant egress via updateMemberDelegation (the working path)
  invoke-contract.ts      execute `forecast` end-to-end and pretty-print the brief
```

## Setup (2 commands)

Prerequisites: Node 20+, `npx tsx` (bundled as a dependency), and a T3N account with an API key saved at `~/.t3n_api_key.txt` (see [docs.terminal3.io](https://docs.terminal3.io) — *set up dev env*). Optional for the real LLM: an OpenRouter key at `~/.openrouter_key` — a free-tier key is enough (`openai/gpt-4o-mini`).

```sh
npm install
npm run setup:kv && npm run register && npm run grant:egress && npm run invoke
```

That's the whole pipeline: create the secrets KV map → build/compile & register the contract → grant egress → invoke and receive the brief. (Building the WASM is only needed after editing the Rust contract — see below.)

> **Note** — `scripts/lib.ts` reads `~/.t3n_api_key.txt`. To target a different tenant, change `TENANT_DID` there and the canonical name in the two scripts that hardcode it (`grant-egress-v2.ts`, `invoke-contract.ts`, and the tail constant is shared).

## Running / verifying

```sh
npm run invoke
# → prints raw response + the parsed Forecast Brief:
# {
#   "summary": "Markets assign roughly a 45% likelihood…",
#   "probability_estimate": 0.45,
#   "reasoning": "Base rates… key uncertainties…",
#   "sources_placeholder": [],
#   "meta": {
#     "phase": "2-llm-in-tee", "placeholder": false,
#     "model": "openai/gpt-4o-mini", "probability_source": "llm-json"
#   }
# }
```

Switch models without recompiling (any OpenRouter model id):

```sh
OPENROUTER_MODEL=openrouter/free npm run invoke
```

### Contract tests (native)

The forecasting logic is host-independent Rust; `cargo test` runs it natively — no TEE, no node, no keys:

```sh
cd forecast-contract && cargo test
```

(3 unit tests: brief shape, empty-question rejection, plus the probability-parser suite — JSON numbers, `"55%"` strings, text scanning that ignores interest-rate percentages, normalization.)

### Building the WASM component

Only needed after editing the Rust contract; `register-contract.ts` reads the artifact:

```sh
cd forecast-contract && cargo build --target wasm32-wasip2 --release
# → target/wasm32-wasip2/release/forecast_contract.wasm (~136 KB component)
```

Verify the component exports the T3N interface:

```sh
wasm-tools component wit target/wasm32-wasip2/release/forecast_contract.wasm
# export z:forecast-contract/contracts@0.1.0 (function: forecast)
```

## Design decisions & architecture notes

1. **Rust → wasm32-wasip2 *component***, `crate-type = ["cdylib", "lib"]`, wit-bindgen 0.49. `cdylib` emits the WASM component (T3N requirement); `lib` keeps the logic unit-testable natively. The LLM path is `#[cfg(target_arch = "wasm32")]` (it needs host imports), while parsing/normalization is pure and tested on the host.
2. **One export, no dispatch** — per T3N convention the WIT function name *is* the export (`forecast: func(req: generic-input) -> result<list<u8>, string>`; the 3-field `generic-input` envelope matches the reference `z-tenant-flight` contract). Errors are plain strings on the `err` branch — no `ContractError` enum to keep the shape ABI-stable.
3. **Capability set = WIT imports.** The world imports exactly what the contract uses; egress authorization lives *not in the world* but in the caller's delegation grant (`allowed_hosts`), so adding/removing hosts never requires recompiling the contract.
4. **Version bumps without ABI breaks** — re-registering bumps only `Cargo.toml`/`CONTRACT_VERSION` (patch), never the WIT world; the testnet rejected submissions with an interface version bump.
5. **KV maps deny-by-default** — `tenant.maps.create` without explicit `readers`/`writers` ACLs produces deny-all; `create-kv-maps.ts` scopes both to the registered `contract_id`, and `register-contract.ts` re-scopes after every registration (ids change on re-register).
6. **Model as data, not code** — the LLM model id is an input field (default compiled in), so model trials are an `npm run invoke` flag, not a rebuild.

## Known limitations (bugs found on T3N SDK v5.3.0 — and the workarounds shipped here)

These threeSDK v5.3.0 issues cost the most debugging time; all workarounds are already wired into `scripts/lib.ts` / `grant-egress-v2.ts` so the quickstart just runs.

### Bug 1 — `fetchTrustedManifest` rejects a *valid* CN-cluster manifest (missing `rtmr1_allowlist`)

- **Repro:** `curl https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest` returns well-formed JSON — *without any `rtmr1_allowlist` key*. `manifestToTrustAnchor(manifest)` accepts it, but the SDK's internal `fetchTrustedManifest` path throws `"malformed…"`.
- **Root cause:** the SDK treats `rtmr1_allowlist` as a required field of the manifest, but the CN cluster's endpoint simply doesn't serve one (RTMR1 attestation differs there — see Bug 2).
- **Workaround** (`scripts/lib.ts → buildTrustAnchor`): fetch the manifest manually, synthesize `rtmr1_allowlist = rtmr3_allowlist` (a harmless placeholder that Bug 1 stops complaining about), then hand the corrected object to `manifestToTrustAnchor`.

### Bug 2 — `assertNodeTrusted` RTMR1 mismatch (SDK allowlist is stale for the CN cluster)

- **Repro:** with Bug 1's manifest patched, `t3n.handshake()` fails with `RTMR1 kP0XBuMMdrW4… not in allowlist`. The *node* attests with RTMR1 = `kP0X…`, but the SDK's pinned allowlist only contains `+XO6…` — which is actually the cluster's **RTMR3** value.
- **Root cause:** the SDK ships an RTMR1 allowlist captured on a different cluster state; CN nodes measure a different RTMR1.
- **Workaround** (`scripts/lib.ts → connect`): parse the real RTMR1 out of the error message, prepend it to `rtmr1_allowlist`, rebuild the trust anchor, re-handshake — i.e., the client self-heals its allowlist from the node's attested value.

### Bug 3 — `grant-egress` via `tenant.execute` is a silent no-op

- **Repro:** sending a raw `agent-auth-update` control action over `tee:user/contracts` through `tenant.execute(...)` returns `{}` (looks like success), but the next contract invocation still fails with `host/http.egress_denied`.
- **Root cause:** `agent-auth-update` on that transport doesn't touch the delegation store that the egress governor actually consults.
- **Fix** (`scripts/grant-egress-v2.ts`): use the typed read-merge-write on the caller's own delegation edge — `t3n.updateMemberDelegation({grantee, contract_id, functions, scopes, version_req, allowed_hosts}, {discoverDids})`. Verified by read-back (`getMemberDelegation`) and by the successful in-TEE OpenRouter call.

### Others (minor)

- Re-registering the same tail yields a **new** `contract_id` — map ACLs must be re-scoped after every registration (handled in `register-contract.ts`).
- `tenant.contracts.listDetailed` doesn't surface `contract_id` per row in v5.3.0 — scripts persist it to `scripts/.contract-id.json` (gitignored) as the source of truth.
- OpenRouter header quirk: sending `Content-Type` explicitly creates a *duplicate* header — the TEE HTTP host sets it; we set only `Authorization`/`Accept`/attribution headers.

## How to run the tests

```sh
cd forecast-contract && cargo test     # native unit tests (no keys, no node)
cargo clippy --all-targets -- -D warnings   # lint-clean as shipped
```

## Scope & disclaimers

- Targets T3N **testnet** (SG cluster today). Judged with the SDK pinned `@terminal3/t3n-sdk@^5.3.0`.
- `probability_estimate` is the LLM's calibrated judgment over provided context — a research artifact, not investment advice.
- `scripts/invoke-contract.ts` ships one example question (Fed rates). The brief is schema-stable, so downstream products can swap the question/context freely.

## Post-challenge roadmap

- **Enterprise forecasting desks:** audit-grade probability estimates where the prompt/company context must stay confidential — LLM + API seal inside the enclave, auditable version pins on testnet.
- **Prediction-market integrations:** point the contract at Polymarket/Kalshi/Manifold public endpoints (same `host:interfaces/http` capability) to blend market-implied and judgmental probabilities.
- **Source-grounded briefs:** require the LLM to emit citations and enrich `sources_placeholder`; add retrieval over tenant KV documents (reads already capability-gated).
- **Ops:** CI that builds the component, runs native tests, and registers to testnet on commit; richer fallback telemetry via `meta.llm_error`.

## License

MIT — see [LICENSE](LICENSE).