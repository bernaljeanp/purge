// Builds a signed installer and publishes it as a GitHub release the in-app updater can see.
//   node scripts/release.mjs "What changed"
// Needs: gh (logged in) and the signing key at %USERPROFILE%\.tauri\purge.key (never commit it).
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const notes = process.argv[2] ?? "";
const { version } = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const env = {
  ...process.env,
  TAURI_SIGNING_PRIVATE_KEY: readFileSync(join(homedir(), ".tauri", "purge.key"), "utf8"),
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "",
};
execSync("npm run tauri build", { stdio: "inherit", env });

const exe = `src-tauri/target/release/bundle/nsis/Purge_${version}_x64-setup.exe`;
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(`${exe}.sig`, "utf8"),
      url: `https://github.com/bernaljeanp/purge/releases/download/v${version}/Purge_${version}_x64-setup.exe`,
    },
  },
};
writeFileSync("latest.json", JSON.stringify(latest, null, 2));
execSync(`gh release create v${version} "${exe}" latest.json --title "Purge v${version}" --notes-file -`, {
  stdio: ["pipe", "inherit", "inherit"],
  input: notes,
});
console.log(`Published v${version}`);
