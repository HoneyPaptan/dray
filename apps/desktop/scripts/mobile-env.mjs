import { execFileSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const OUT = new URL("../.env.mobile.local", import.meta.url);

// Vite's own `.local` convention, and the root .gitignore already drops
// `*.local` — which is what keeps the token out of the repository.
function tailnetAddress() {
  try {
    const out = execFileSync("tailscale", ["ip", "-4"], { encoding: "utf8" });
    return out.split("\n").map((line) => line.trim()).find(Boolean) ?? null;
  } catch {
    return null;
  }
}

function token() {
  try {
    return readFileSync(join(homedir(), ".dray", "remote-token"), "utf8").trim() || null;
  } catch {
    return null;
  }
}

// The mode rides the create, the same reading `~/.dray/remote-token`'s own
// write takes: this file carries that token, and `writeFileSync` leaves an
// existing file's mode alone, so a previously world-readable copy is removed
// rather than written over.
function writeOwnerOnly(contents) {
  rmSync(OUT, { force: true });
  writeFileSync(OUT, contents, { mode: 0o600 });
}

// The same variable `serve.rs` reads, so the baked port cannot drift from the
// one the desktop listens on.
const port = process.env.DRAY_SERVE_PORT ?? "8787";
const address = tailnetAddress();
const secret = token();

if (!address || !secret) {
  // Written empty rather than left stale: a build made on a machine with no
  // tailnet or no token must not ship the previous machine's endpoint.
  writeOwnerOnly("");
  const missing = !address ? "no tailnet address" : "no ~/.dray/remote-token";
  console.warn(`[mobile-env] ${missing}, so the app will ask for a host on first run`);
} else {
  writeOwnerOnly(`VITE_DRAY_URL=ws://${address}:${port}\nVITE_DRAY_TOKEN=${secret}\n`);
  console.log(`[mobile-env] baked ws://${address}:${port}`);
}
