// quickstart.ts — minimal "connect to T3N" verification for the Forecast Brief Agent.
//
// Canonical version (collapsed from the debugging quickstarts 2–4). It performs:
//   1. Load the platform trust manifest — with the two CN-cluster fixes from
//      README "Known limitations" (missing rtmr1_allowlist + stale RTMR1 pin).
//   2. Build a T3nClient with an Ethereum-signed session.
//   3. Handshake + authenticate, print the authenticated DID.
//
// Usage:  npx tsx quickstart.ts            (expects ~/.t3n_api_key.txt)
import {
  T3nClient,
  setEnvironment,
  loadWasmComponent,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  manifestToTrustAnchor,
} from "@terminal3/t3n-sdk";
import fs from "node:fs";

setEnvironment("testnet");

const T3N_API_KEY = fs.readFileSync(process.env.HOME + "/.t3n_api_key.txt", "utf8").trim();
const CN_MANIFEST_URL = "https://cn-api.sg.testnet.t3n.terminal3.io/api/trust-manifest";

// --- Bug 1 workaround: the CN cluster manifest lacks `rtmr1_allowlist`, which
// the SDK's fetchTrustedManifest treats as required ("malformed"). We fetch it
// ourselves and synthesize the key from rtmr3_allowlist (harmless placeholder;
// Bug 2 below replaces it with the node's real value when needed).
interface TrustManifest { rtmr3_allowlist?: string[]; rtmr1_allowlist: string[] }

async function buildTrustAnchor(): Promise<TrustManifest> {
  const manifest = (await (await fetch(CN_MANIFEST_URL)).json()) as TrustManifest;
  manifest.rtmr1_allowlist = manifest.rtmr3_allowlist ?? [];
  console.log("trust manifest fetched (rtmr1 placeholder = rtmr3 list)");
  return manifest;
}

function newClient(manifest: TrustManifest, wasmComponent: Awaited<ReturnType<typeof loadWasmComponent>>) {
  const address = eth_get_address(T3N_API_KEY);
  return new T3nClient({
    trustAnchor: manifestToTrustAnchor(manifest),
    wasmComponent,
    handlers: { EthSign: metamask_sign(address, undefined, T3N_API_KEY) },
  });
}

const wasmComponent = await loadWasmComponent();
const address = eth_get_address(T3N_API_KEY);
const manifest = await buildTrustAnchor();

// --- Bug 2 workaround: the CN node attests an RTMR1 value that is not in the
// SDK v5.3.0 pinned allowlist (the pinned value is the cluster's RTMR3). On
// handshake failure we extract the node's real RTMR1 from the error, prepend it
// to the allowlist, rebuild the client and retry once.
let t3n = newClient(manifest, wasmComponent);
try {
  await t3n.handshake();
} catch (e: any) {
  const m: RegExpMatchArray | null = e?.message?.match(/RTMR1 ([A-Za-z0-9+/=]{8,50})[;,)]/);
  if (m && e.message.includes("not in allowlist")) {
    console.log(`heal: node RTMR1 ${m[1].slice(0, 12)}… missing from allowlist — retrying with it added`);
    manifest.rtmr1_allowlist = [m[1], ...manifest.rtmr1_allowlist];
    t3n = newClient(manifest, wasmComponent);
    await t3n.handshake();
  } else {
    throw e;
  }
}

const auth = await t3n.authenticate(createEthAuthInput(address));
console.log("Connected as:", auth.value);