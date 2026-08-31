// grant-egress-v2.ts — self-grant de egress vía la API tipada del SDK.
//
// El intent v1 (raw tenant.execute con agent-auth-update sobre tee:user/contracts)
// devolvió {} pero NO tocó el store de delegación: la invocación siguió saliendo
// con host/http.egress_denied. La vía correcta per index.d.ts es read-merge-write
// de la delegation edge del llamador (SelfOnly → el tenant escribe su propia
// edge) con un BoundGrant: grantee = DID propio, contract_id = canonical del
// forecast contract, allowed_hosts = ["openrouter.ai"].
import { connectTenant, getContractVersion, NODE_URL } from "./lib.ts";

const CANONICAL = "z:cc2ee922b9d2328c98aebf6f97c1d36b7814ebaa:forecast-contract";

const session = await connectTenant();
const { t3n, did } = session;

const scriptVersion = await getContractVersion(NODE_URL, CANONICAL);
console.log("grant v2:", did, "→", CANONICAL, "v" + scriptVersion, "hosts: [openrouter.ai]");

// 1) Leer el documento actual (para no patear grants existentes).
let current: any = null;
try {
  current = await (t3n as any).getMemberDelegation();
  console.log("delegation doc actual:", JSON.stringify(current).slice(0, 500));
} catch (e: any) {
  console.log("getMemberDelegation falló (puede ser primera vez):", e.message ?? e);
  // try legacy reader
  try {
    current = await (t3n as any).getAgentAuth();
    console.log("legacy agent-auth doc:", JSON.stringify(current).slice(0, 500));
  } catch (e2: any) {
    console.log("getAgentAuth también falló:", e2.message ?? e2);
  }
}

// 2) Escribir el grant con read-merge-write tipado o legacy.
const grant: any = {
  grantee: did,
  contract_id: CANONICAL,
  functions: ["forecast"],
  scopes: [],
  version_req: scriptVersion,
  allowed_hosts: ["openrouter.ai"],
};

try {
  const res = await (t3n as any).updateMemberDelegation(grant, { discoverDids: [did] });
  console.log("✅ grant OK (member-delegation-update):", JSON.stringify(res ?? {}).slice(0, 400));
} catch (e: any) {
  console.error("updateMemberDelegation falló:", e.message ?? e);
  if (e.request_id) console.error("request_id:", e.request_id);
  // Fallback: legacy camelCase con merge propio
  try {
    const existing = current?.grants ?? current?.agents ?? [];
    if (Array.isArray(existing) && existing.some((g: any) => g.agentDid || g.grantee)) {
      const res = await (t3n as any).updateAgentAuth(did, {
        scriptName: CANONICAL,
        versionReq: scriptVersion,
        functions: ["forecast"],
        allowedHosts: ["openrouter.ai"],
      });
      console.log("✅ grant OK (legacy updateAgentAuth):", JSON.stringify(res ?? {}).slice(0, 400));
    } else {
      const res = await (t3n as any).agentAuthUpdate({
        agents: [{
          agentDid: did,
          scripts: [{
            scriptName: CANONICAL,
            versionReq: scriptVersion,
            functions: ["forecast"],
            allowedHosts: ["openrouter.ai"],
          }],
        }],
        discoverDids: [did],
      });
      console.log("✅ grant OK (legacy agentAuthUpdate):", JSON.stringify(res ?? {}).slice(0, 400));
    }
  } catch (e2: any) {
    console.error("Fallback legacy también falló:", e2.message ?? e2);
    if (e2.request_id) console.error("request_id:", e2.request_id);
    process.exit(1);
  }
}

// 3) Verificación de lectura post-write.
try {
  const after: any = await (t3n as any).getMemberDelegation();
  console.log("delegation doc tras write:", JSON.stringify(after).slice(0, 600));
} catch (e: any) {
  console.log("read-back falló:", e.message ?? e);
}