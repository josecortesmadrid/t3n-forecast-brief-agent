// invoke-contract.ts — per https://docs.terminal3.io/developers/adk/get-started/walkthrough/invoke-contract
// Direct (self) call: the tenant invokes its own contract — no separate agent
// session or agent-auth grant is needed because phase 1 makes NO outbound HTTP
// calls (egress only matters when the contract dials hosts; self-grant would
// only be needed for host/http).
import { connectTenant, getContractVersion, NODE_URL } from "./lib.ts";

const CANONICAL = "z:cc2ee922b9d2328c98aebf6f97c1d36b7814ebaa:forecast-contract";
const CONTRACT_TAIL = "forecast-contract";

const session = await connectTenant();
const { tenant, t3n } = session;

const contractVersion = await getContractVersion(NODE_URL, CANONICAL);
console.log("versión registrada:", contractVersion);

// Fase 2: el contract lee llm_api_key de z:<tid>:secrets y llama a OpenRouter
// dentro de la TEE. OPENROUTER_MODEL permite cambiar de modelo sin recompilar
// (paso 8: probar openrouter/free si gpt-4o-mini no tiene créditos).
const input: Record<string, unknown> = {
  question: "Will the Federal Reserve cut rates before October 2026?",
  context:
    "FOMC 2026 meeting schedule: Sep 15-16 and Oct 27-28. Inflation has cooled for three consecutive months; markets price roughly one cut by year-end.",
};
if (process.env.OPENROUTER_MODEL) {
  input.model = process.env.OPENROUTER_MODEL;
  console.log("modelo override vía OPENROUTER_MODEL:", process.env.OPENROUTER_MODEL);
}

console.log("invocando forecast — input:", JSON.stringify(input));

// Execute as the tenant itself (self call). Prefer the typed
// tenant.contracts.execute(tail, {version, functionName, input}); fall back to
// the raw execute transport with the canonical contract_id.
let raw: string | unknown;
try {
  raw = await tenant.contracts.execute(CONTRACT_TAIL, {
    version: contractVersion,
    functionName: "forecast",
    input,
  });
} catch (e: any) {
  console.log("tenant.contracts.execute falló (" + (e.message ?? e) + ") — probando transporte crudo…");
  raw = await t3n.execute({
    contract_id: CANONICAL,
    contract_version: contractVersion,
    function_name: "forecast",
    input,
  });
}
console.log("\nraw response:\n", raw);

// Parse + print the brief.
try {
  const brief = typeof raw === "string" ? JSON.parse(raw) : raw;
  const payload = (brief as any)?.result ?? (brief as any)?.output ?? brief;
  const decoded = typeof payload === "string" ? JSON.parse(payload) : payload;
  console.log("\n=== FORECAST BRIEF ===");
  console.log(JSON.stringify(decoded, null, 2));
} catch (e: any) {
  console.error("no se pudo parsear el brief como JSON:", e.message);
  console.error("(el contract pudo devolver un error de negocio — ver raw arriba)");
}