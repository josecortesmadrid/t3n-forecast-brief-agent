// create-kv-maps.ts — per https://docs.terminal3.io/developers/adk/tips/create-kv-maps
// Creates the `secrets` map ACL'd to the forecast contract so its kv-store reads
// aren't denied by the kv-governor (readers default to DENY when omitted).
// idempotent: MapAlreadyExists on re-run is safe per docs common-errors.
import { readFile } from "node:fs/promises";
import { connectTenant } from "./lib.ts";

const CONTRACT_TAIL = "forecast-contract";

const session = await connectTenant();
const { tenant, t3n } = session;

// contract_id is required to scope the map's writers/readers to the contract.
// Check whether the contract is already registered (listDetailed gives details
// without re-registering).
let contractId: number | null = null;
try {
  const page = await (tenant.contracts as any).listDetailed({ limit: 50 });
  for (const row of page.contracts) {
    console.log(`  contrato existente: ${row.name} v${row.version} [${row.status}]`);
    if (row.name.endsWith(":" + CONTRACT_TAIL)) {
      contractId = Number((row as any).contract_id ?? NaN);
    }
  }
} catch (e: any) {
  console.log("  listDetailed no disponible o falló:", e.message);
}

// Fallback: read id saved by register-contract.ts (.contract-id.json)
if (contractId == null || Number.isNaN(contractId)) {
  try {
    const saved = JSON.parse(await readFile(new URL("./.contract-id.json", import.meta.url), "utf8"));
    if (saved.tail === CONTRACT_TAIL && typeof saved.contract_id === "number") {
      contractId = saved.contract_id;
    }
  } catch {
    // no saved id — maps will be created unscoped, register-contract.ts re-scopes later
  }
}

const mapTail = "secrets";
console.log(`creando map "z:<tid>:${mapTail}" (contract_id=${contractId ?? "?"}) …`);
try {
  // Docs: readers MUST be explicit — kv-governor defaults to deny-all reads.
  await (tenant.maps as any).create({
    tail: mapTail,
    visibility: "private",
    writers: contractId != null ? { only: [contractId] } : { only: [] },
    readers: contractId != null ? { only: [contractId] } : { only: [] },
  });
  console.log("  map creado OK");
} catch (e: any) {
  if (/map already exists/i.test(e.message ?? "")) {
    console.log("  map ya existe — idempotente OK");
  } else {
    console.error("map create error:", e.message);
    if ((e as any).request_id) console.error("request_id:", (e as any).request_id);
    process.exit(1);
  }
}

if (contractId == null) {
  console.log("  AVISO: sin contract_id el ACL quedó sin escoper — re-ejecutar después de register-contract.ts.");
}

console.log("\nseeding llm_api_key en secrets (control-plane write, bypassa writers ACL)…");
let llmKey = "";
try {
  llmKey = (await readFile(process.env.HOME + "/.openrouter_key", "utf8")).trim();
  console.log("  usando OpenRouter key de ~/.openrouter_key (no se imprime)");
} catch {
  console.log("  .openrouter_key no encontrada — placeholder (fase 1 no llama LLM)");
  llmKey = "sk-or-placeholder-phase1";
}
try {
  await tenant.executeControl("map-entry-set", {
    map_name: tenant.canonicalName(mapTail),
    key: "llm_api_key",
    value: llmKey,
  });
  console.log("  llm_api_key sellada en z:<tid>:secrets");
} catch (e: any) {
  try {
    await tenant.maps.entrySet(mapTail, "llm_api_key", llmKey);
    console.log("  llm_api_key sellada (vía maps.entrySet)");
  } catch (e2: any) {
    console.error("seed error:", e2.message ?? e2);
    process.exit(1);
  }
}

console.log("\nOK: maps + seed listos.");