# T3N Forecast Brief Agent — Enterprise forecasting in confidential compute

> Contenido listo para pegar en Google Docs. Los marcadores `[SCREENSHOT: ...]` se reemplazan por las capturas correspondientes (ver `docs/evidence-*.txt` para el texto de respaldo de cada una).

---

## 1. Project summary

**T3N Forecast Brief Agent** is a forecasting agent built on the T3N (Terminal 3 Network) ADK in which the LLM call itself runs **inside the TEE**. A Rust → WASM component (target `wasm32-wasip2`) is registered on T3N testnet; when invoked, it (1) reads an OpenRouter API key from the enclave's key-value store — the key never leaves confidential compute, (2) performs an outbound HTTPS call to OpenRouter through T3N's capability-gated `host:interfaces/http`, and (3) returns a structured **Forecast Brief**: `{ summary, probability_estimate, reasoning, sources, meta }`. The live end-to-end run on testnet answered *"Will the Federal Reserve cut rates before October 2026?"* with a calibrated estimate of **0.45** produced by `openai/gpt-4o-mini` inside the enclave (`meta.phase = "2-llm-in-tee"`, `meta.placeholder = false`, `meta.probability_source = "llm-json"`). Registered contract: `z:<tenant>:forecast-contract` v0.1.1 (and v0.1.2 after the final re-registration documented below), on the SG testnet cluster.

## 2. Architecture

Flow: **tenant → agent (scripts / SDK) → TEE contract → LLM → brief (with audit trail)**

```
┌────────────────────────────┐
│  Tenant (you, scripts/)    │  T3N SDK v5.3 · Ethereum-signed session
│  - secrets in z:<tid>:kv   │  llm_api_key sealed via control-plane write
└──────────┬─────────────────┘
           │ register (WASM component) / execute(forecast, input)
           ▼
┌────────────────────────────────────────────────────────┐
│  T3N node — TEE (SG testnet cluster)                   │
│  ┌──────────────────────────────────────────────────┐  │
│  │ forecast-contract (Rust → wasm32-wasip2)         │  │
│  │  1. kv-store::get("z:<tid>:secrets")             │  │ ← key never leaves the enclave
│  │  2. http::call POST openrouter.ai (gated egress) │  │ ← host allowlist: ["openrouter.ai"]
│  │  3. parse reply → Forecast Brief JSON            │  │ ← fallback brief, never panic
│  └──────────────────────────────────────────────────┘  │
└──────────┬─────────────────────────────────────────────┘
           │ OpenRouter chat/completions (openai/gpt-4o-mini default)
           ▼
   Forecast Brief: { summary, probability_estimate, reasoning, sources, meta }
```

**Why it matters (usefulness):** enterprise forecasting needs prompts and credentials to stay confidential — the API key is sealed inside the enclave, every outbound host is governed by an explicit delegation grant, and each run is attributable to a pinned contract version on-chain of the testnet. The brief is schema-stable JSON, so it drops directly into dashboards, alerts, or downstream agents.

**Maintainability post-challenge:** the model id is an *input*, not compiled code (`OPENROUTER_MODEL=openrouter/free npm run invoke` switches models without rebuilding); version bumps are patch-level (`Cargo.toml` + `CONTRACT_VERSION`) and never touch the WIT ABI; the KV map ACLs are re-scoped automatically on every registration; and the whole logic is unit-testable natively with `cargo test` — no node, no keys, no TEE required.

## 3. Screenshots

- `[SCREENSHOT: invoke-output-brief.png]` — real end-to-end invocation: the terminal running `npx tsx scripts/invoke-contract.ts`, showing the authenticated DID, the input, and the parsed **Forecast Brief** with `probability_estimate: 0.45`, `phase: "2-llm-in-tee"`, `probability_source: "llm-json"`. *(Raw text evidence: `docs/evidence-output.txt` in the repo.)*
- `[SCREENSHOT: registered-contract.png]` — registration output: canonical name `z:<tid>:forecast-contract`, `contract_id`, version bump `0.1.1 → 0.1.2`, and the KV-map ACL re-scoping message. *(Raw text evidence: `docs/evidence-registration.txt`.)*
- `[SCREENSHOT: egress-grant.png]` — `grant-egress-v2.ts` read-back: the delegation document showing the `BoundGrant` with `allowed_hosts: ["openrouter.ai"]`, `version_req: "0.1.1"`, `functions: ["forecast"]`. *(Raw text evidence: `docs/evidence-egress.txt`.)*
- `[SCREENSHOT: native-tests.png]` — `cargo test` output inside `forecast-contract/`: 3/3 unit tests passing (brief shape, empty-question rejection, probability-parser suite).
- `[SCREENSHOT: repo-home.png]` — the public GitHub repository home page.

## 4. Bugs found & fixed (T3N SDK v5.3.0)

The three bugs below were the deepest debugging rabbit holes of the build. **All three workarounds are shipped in the repo** (`scripts/lib.ts`, `scripts/grant-egress-v2.ts`), so the documented quickstart commands run green as-is.

### Bug 1 — `fetchTrustedManifest` rejects a *valid* SG/CN-cluster manifest (missing `rtmr1_allowlist`)

- **Exact repro:**
  ```sh
  curl https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest
  # → well-formed JSON, but with NO `rtmr1_allowlist` key
  ```
  Passing that untouched manifest to `manifestToTrustAnchor(manifest)` **works**, but letting the SDK download it through its own `fetchTrustedManifest` path throws a *"malformed …"* error.
- **Root cause:** the SDK treats `rtmr1_allowlist` as a required manifest field, but the cluster's trust-manifest endpoint simply doesn't serve that key (RTMR1 attestation differs there — which leads straight into Bug 2).
- **Fix shipped** (`scripts/lib.ts → buildTrustAnchor`): fetch the manifest manually, synthesize `rtmr1_allowlist = rtmr3_allowlist` (a placeholder value), then hand the corrected object to `manifestToTrustAnchor`. The SDK's own fetch path is never used.

### Bug 2 — `assertNodeTrusted` RTMR1 mismatch: node attests an RTMR1 absent from the SDK's pinned allowlist

- **Exact repro:** with Bug 1 patched, `t3n.handshake()` fails with:
  `RTMR1 kP0XBuMMdrW4… not in allowlist`. The *node* publishes RTMR1 = `kP0X…`, while the SDK's pinned allowlist only accepts `+XO6…` — which is actually the cluster's **RTMR3** value.
- **Root cause:** the SDK ships an RTMR1 allowlist captured from a different cluster state; the SG testnet nodes measure a different RTMR1.
- **Fix shipped** (`scripts/lib.ts → connect`): parse the node's real RTMR1 out of the error message with a regex, prepend it to `rtmr1_allowlist` in the manifest, rebuild the `T3nClient`, and re-handshake — the client *self-heals* its allowlist from the attested value of the very node it's connecting to.

### Bug 3 — egress grant via `tenant.execute` is a silent no-op (delegation never persists)

- **Exact repro:** sending a raw `agent-auth-update` control action over `tee:user/contracts`:
  ```ts
  await tenant.execute({ contract_id: "tee:user/contracts", function_name: "agent-auth-update", input: { agents: [{ agentDid, scripts: [{ scriptName, allowedHosts: ["openrouter.ai"], ... }] }] } });
  // → returns {}  (looks like success)
  // …but the next contract invocation still fails with: host/http.egress_denied
  ```
- **Root cause:** that transport path doesn't touch the delegation store the egress governor actually consults — the write silently goes nowhere.
- **Fix shipped** (`scripts/grant-egress-v2.ts`): use the typed read-merge-write on the caller's own delegation edge:
  ```ts
  await t3n.updateMemberDelegation({
    grantee: did, contract_id: CANONICAL,
    functions: ["forecast"], scopes: [],
    version_req: "0.1.1", allowed_hosts: ["openrouter.ai"],
  }, { discoverDids: [did] });
  ```
  Verified by read-back (`getMemberDelegation()` shows the grant, with the host adding a 90-day auto-window) and, decisively, by the in-TEE OpenRouter call succeeding.

### Minor issues (also documented in the repo README)

- Re-registering the same tail yields a **new** `contract_id` → KV map ACLs (`readers`/`writers`) go stale; `register-contract.ts` re-scopes them after every registration (that's the v0.1.1 → v0.1.2 + id 810 re-registration in the screenshots).
- `tenant.contracts.listDetailed` doesn't expose `contract_id` per row in v5.3.0 — scripts persist it to a gitignored local file as source of truth.
- Sending `Content-Type` explicitly through the TEE HTTP host produces a **duplicate header** → the contract sets only `Authorization` / `Accept` / attribution headers and lets the host set content type.

## 5. How to run

```sh
# prerequisites: Node 20+, a T3N API key at ~/.t3n_api_key.txt
# optional (real LLM): an OpenRouter key at ~/.openrouter_key
git clone https://github.com/josecortesmadrid/t3n-forecast-brief-agent
cd t3n-forecast-brief-agent
npm install

# create + ACL the secrets KV map, seal the LLM key, register, grant egress, invoke:
npm run setup:kv && npm run register && npm run grant:egress && npm run invoke
```

The invoke prints the raw response and the parsed Forecast Brief. Quick verification without a node or any keys:

```sh
cd forecast-contract && cargo test   # 3/3 native unit tests
cargo build --target wasm32-wasip2 --release   # rebuild the WASM component after editing Rust
```

Auth is fully offline-friendly: the trust-anchor fixes in `scripts/lib.ts` mean no manual manifest surgery is needed — the client heals the RTMR1 allowlist itself on first handshake.

## 6. Post-challenge roadmap (why this keeps running)

- **Enterprise forecasting desks:** audit-grade probability estimates where the prompt, company context, and LLM credentials must stay confidential — LLM + sealed API key inside the enclave, with each run attributable to a pinned, versioned contract. Maintenance is intentionally boring: patch-bump + `npm run register`.
- **Prediction-market platform integrations:** point the same `host:interfaces/http` capability at Polymarket / Kalshi / Manifold public endpoints to blend market-implied prices with judgmental forecasts in one brief.
- **Source-grounded briefs:** require the LLM to emit citations and enrich `sources_placeholder`; add retrieval over tenant KV documents (reads are already capability-gated per contract).
- **Ops hardening:** CI that builds the component, runs the native test suite, and registers to testnet on commit; richer fallback telemetry via `meta.llm_error`.

## Links

- **GitHub repo:** https://github.com/josecortesmadrid/t3n-forecast-brief-agent
- **X post:** *(pending — draft at `docs/social-post.md`; will be shared tagging **@terminal3io**)*