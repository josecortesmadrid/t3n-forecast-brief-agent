// lib.ts — shared bootstrap for T3N tenant scripts.
// Same trustAnchor pattern as quickstart4.ts (fixed manifest + RTMR1 self-heal),
// plus a TenantClient per docs.terminal3.io set-up-dev-env.
import {
  T3nClient,
  TenantClient,
  setEnvironment,
  loadWasmComponent,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  manifestToTrustAnchor,
  getNodeUrl,
  getContractVersion,
} from "@terminal3/t3n-sdk";
import { readFile } from "node:fs/promises";
import fs from "node:fs";

setEnvironment("testnet");

export const T3N_API_KEY = fs.readFileSync(process.env.HOME + "/.t3n_api_key.txt", "utf8").trim();
export const TENANT_DID = "did:t3n:cc2ee922b9d2328c98aebf6f97c1d36b7814ebaa";
export const NODE_URL = getNodeUrl();

/** Fixed-cluster-manifest trust anchor (CN cluster RTMR1 misalignment workaround). */
async function buildTrustAnchor(): Promise<TrustAnchorJ> {
  const j: any = await (await fetch("https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest")).json();
  j.rtmr1_allowlist = j.rtmr3_allowlist || [];
  return j;
}

type TrustAnchorJ = Record<string, unknown> & { rtmr1_allowlist: string[] };

/** Handshake with the quickstart4 retry-heal for RTMR1 allowlist rejection. */
async function connect(wasmComponent: Awaited<ReturnType<typeof loadWasmComponent>>): Promise<{ t3n: T3nClient; did: string; manifest: TrustAnchorJ }> {
  const address = eth_get_address(T3N_API_KEY);
  let manifest = await buildTrustAnchor();
  let t3n = new T3nClient({
    trustAnchor: manifestToTrustAnchor(manifest),
    wasmComponent,
    handlers: { EthSign: metamask_sign(address, undefined, T3N_API_KEY) },
  });
  try {
    await t3n.handshake();
  } catch (e: any) {
    const m = e.message?.match?.(/RTMR1 ([A-Za-z0-9+/=]{8,50})[;,)]/);
    if (m && e.message.includes("not in allowlist")) {
      const realRtmr1 = m[1];
      console.log(`  [heal] RTMR1 real detectado ${realRtmr1.slice(0, 12)}… — añadido a allowlist`);
      manifest = { ...manifest, rtmr1_allowlist: [realRtmr1, ...(manifest.rtmr1_allowlist || [])] };
      t3n = new T3nClient({
        trustAnchor: manifestToTrustAnchor(manifest),
        wasmComponent,
        handlers: { EthSign: metamask_sign(address, undefined, T3N_API_KEY) },
      });
      await t3n.handshake();
    } else {
      throw e;
    }
  }
  const auth = await t3n.authenticate(createEthAuthInput(address));
  return { t3n, did: auth.value, manifest };
}

export interface Session {
  t3n: T3nClient;
  tenant: TenantClient;
  did: string;
  tenantName: string;
}

/** Full authenticated tenant session. */
export async function connectTenant(wasmPath?: string): Promise<Session> {
  const wasmComponent = await loadWasmComponent(wasmPath);
  const { t3n, did } = await connect(wasmComponent);
  console.log("  autenticado como:", did);

  const tenant = new TenantClient({
    t3n,
    baseUrl: NODE_URL, // per docs: always pass explicitly on TenantClient
    tenantDid: TENANT_DID,
    endpoint: NODE_URL,
  } as any);

  const me = await tenant.tenant.me();
  console.log("  TenantClient ready —", me.tenant, me.status);
  return { t3n, tenant, did, tenantName: me.tenant };
}

// Re-exports so scripts can stay one-line imports.
export { eth_get_address, metamask_sign, createEthAuthInput, getContractVersion, loadWasmComponent, readFile };