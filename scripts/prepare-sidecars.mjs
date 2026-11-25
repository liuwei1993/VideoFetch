import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

if (process.platform === "win32") {
  run("powershell.exe", [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    resolve(root, "scripts/fetch-windows-sidecars.ps1"),
  ]);
} else if (process.platform === "linux") {
  run("bash", [resolve(root, "scripts/fetch-linux-sidecars.sh")]);
} else {
  console.error(`No sidecar fetch script for ${process.platform}`);
  process.exit(1);
}
