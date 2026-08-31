// register-contract.ts — per https://docs.terminal3.io/developers/adk/get-started/walkthrough/register-contract
// Registers forecast_contract.wasm under tail "forecast-contract" with the
// authenticated TenantClient. Re-registers bump the version (auto-detected from
// the node's current registered version; see common-errors "version not higher").
import { readFile, writeFile } from "node:fs/promises";
import { connectTenant, getContractVersion, NODE_URL } from "./lib.ts";

const CONTRACT_TAIL = "forecast-contract";
const WASM_PATH = new URL("../forecast-contract/target/wasm32-wasip2/release/forecast_contract.wasm", import.meta.url).pathname;
const ID_PATH = new URL("./.contract-id.json", import.meta.url).pathname;

const canonical = `z:cc2ee922b9d2328c98aebf6f97c1d36b7814ebaa:${CONTRACT_TAIL}`;

const session = await connectTenant();
const { tenant } = session;

// Current registered version on the node (404 → unregistered → start at 0.1.0).
let current: string | null = null;
try {
  current = await getContractVersion(NODE_URL, canonical);
  console.log(`versión actualmente registrada: ${current}`);
} catch (e: any) {
  if (!/404/.test(e.message ?? "")) {
    console.log("No se pudo consultar versión actual:", e.message);
  } else {
    console.log("tail aún no registrado — empezando en 0.1.0");
  }
}

let version = "0.1.0";
if (current) {
  // bump patch
  const [a, b, c] = current.split(".").map(Number);
  version = `${a}.${b}.${(c || 0) + 1}`;
  console.log(`bump: ${current} -> ${version}`);
}

const wasmBytes = await readFile(WASM_PATH);
console.log(`WASM: ${WASM_PATH} (${wasmBytes.length} bytes)`);

try {
  const result = await tenant.contracts.register({ tail: CONTRACT_TAIL, version, wasm: wasmBytes });
  console.log("\n✅ REGISTRADO");
  console.log("   canonical name:", result.name);
  console.log("   contract_id:", result.contract_id);
  console.log("   version:", version);
  await writeFile(
    ID_PATH,
    JSON.stringify({ tail: CONTRACT_TAIL, name: result.name, contract_id: result.contract_id, version, registered_at: new Date().toISOString() }, null, 2) + "\n",
  );
  console.log("   guardado en scripts/.contract-id.json (necesario para ACLs de maps)");

  // Scope the secrets map ACL to this contract (create-kv-maps may have run
  // before any contract existed, leaving the map write-only).
  try {
    await (tenant.maps as any).update("secrets", {
      readers: { only: [result.contract_id] },
      writers: { only: [result.contract_id] },
    });
    console.log("   map 'secrets' ACL rescopiado a contract_id", result.contract_id);
  } catch (aclErr: any) {
    console.log("   ACL update del map 'secrets' falló (no bloqueante en fase 1):", aclErr.message ?? aclErr);
    if ((aclErr as any).request_id) console.log("   request_id:", (aclErr as any).request_id);
  }
} catch (e: any) {
  console.error("\n❌ register falló:", e.message ?? e);
  if (/version .* is not higher/i.test(e.message ?? "")) {
    console.error("   → bump manual de CONTRACT_VERSION y reintentar");
  }
  if ((e as any).request_id) console.error("   request_id:", (e as any).request_id);
  process.exit(1);
}