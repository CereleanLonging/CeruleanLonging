import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const [task, ...args] = process.argv.slice(2);
const windows = process.platform === "win32";
const shell = windows ? join(process.env.SystemRoot || "C:/Windows", "System32/WindowsPowerShell/v1.0/powershell.exe") : "pwsh";
if (task === "ui" && (args.length !== 1 || !/^[a-z0-9-]+\.mjs$/i.test(args[0]))) throw Error("Expected one repository UI test filename");
const commands = {
  ui: [process.execPath, join(root, "tests", args[0] || "")],
  desktop: [process.execPath, join(root, "scripts/desktop.mjs"), ...args],
  frontend: [process.execPath, join(root, "scripts/frontend.mjs"), ...args],
  check: [shell, "-NoProfile", ...(windows ? ["-ExecutionPolicy", "RemoteSigned"] : []), "-File", join(root, "scripts/check.ps1"), "-CacheSession"],
  lint: [shell, "-NoProfile", ...(windows ? ["-ExecutionPolicy", "RemoteSigned"] : []), "-File", join(root, "scripts/lint.ps1"), "-CacheSession"],
  contracts: ["cargo", "run", "-p", "cerulean-domain", "--bin", "export-contracts", "--locked"],
};
if (!commands[task]) throw Error("Unknown managed build/test task");
const schedule = command => task === "ui" ? command : [process.execPath, join(root, "scripts/build-scheduler.mjs"), ...(task === "frontend" ? [] : ["--rust"]), ...command];
const execute = (program, arguments_, options = {}) => {
  const result = spawnSync(program, arguments_, { cwd: root, stdio: "inherit", shell: false, ...options });
  if (result.error) throw result.error;
  return result.status ?? 1;
};

if (process.env.CERULEAN_CACHE_SESSION) {
  const command = schedule(commands[task]);
  process.exit(execute(command[0], command.slice(1)));
}

const extension = windows ? ".exe" : "";
const tools = join(root, "target/cache-tools");
const manager = join(tools, `cerulean-cache${extension}`);
const stamp = join(tools, "source.sha256");
const hash = createHash("sha256");
for (const file of ["Cargo.lock", "crates/cache/Cargo.toml", "crates/cache/src/lib.rs", "crates/cache/src/main.rs", "crates/cache/src/task_job.rs"]) hash.update(readFileSync(join(root, file)));
const fingerprint = hash.digest("hex");
if (!existsSync(manager) || !existsSync(stamp) || readFileSync(stamp, "utf8") !== fingerprint) {
  const bootstrap = join(root, ".temp/build-cache/bootstrap/cargo");
  const result = execute(process.execPath, [join(root, "scripts/build-scheduler.mjs"), "--rust", "cargo", "build", "-p", "cerulean-cache", "--release", "--locked", "--target-dir", bootstrap]);
  if (result) process.exit(result);
  mkdirSync(tools, { recursive: true });
  copyFileSync(join(bootstrap, "release", `cerulean-cache${extension}`), manager);
  writeFileSync(stamp, fingerprint);
}
let result = 1;
try {
  result = execute(manager, ["run", root, ...schedule(commands[task])]);
} finally {
  // Final executables live in target/{debug,release}; only compiler-cache subtrees
  // and managed temporary payloads are candidates for this operation.
  const cleanup = execute(manager, ["collect", root], { stdio: ["ignore", "ignore", "inherit"] });
  if (cleanup && result === 0) result = cleanup;
}
process.exit(result);
