import { spawnSync } from "node:child_process";
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
const mode = process.argv[2];
if (!["dev", "build", "package"].includes(mode)) throw Error("Expected dev, build or package");
if (!process.env.CERULEAN_CACHE_SESSION) {
  const managed = spawnSync(process.execPath, ["scripts/cache-task.mjs", "desktop", mode], { stdio: "inherit", shell: false });
  if (managed.error) throw managed.error;
  process.exit(managed.status ?? 1);
}
const session = resolve(process.env.CERULEAN_CACHE_SESSION.replace(/^\\\\\?\\/, ""));
const sessionRoot = resolve(".temp/build-cache/jobs");
if (!session.startsWith(sessionRoot + (process.platform === "win32" ? "\\" : "/")) ||
    resolve((process.env.CARGO_TARGET_DIR || "").replace(/^\\\\\?\\/, "")) !== join(session, "cargo")) throw Error("Invalid managed compilation directory");
const cargoTarget = join(session, "cargo");
const release = mode !== "dev";
const run = (bin, args, cwd = process.cwd(), env = process.env) => {
  const r = spawnSync(bin, args, { stdio: "inherit", shell: false, cwd, env });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
};
if (mode === "package") {
  const reviewedEnvironment = { ...process.env };
  delete reviewedEnvironment.CARGO_TARGET_DIR;
  run("python", ["scripts/sqlite-notices.py", "--check"], process.cwd(), reviewedEnvironment);
  run("python", ["scripts/distribution-notices.py", "--check"], process.cwd(), reviewedEnvironment);
}
const rust = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
if (rust.status !== 0)
  throw Error(
    "Rust is unavailable. Run scripts/dev.ps1 or add cargo/bin to PATH.",
  );
const triple = rust.stdout.match(/^host: (.+)$/m)?.[1]?.trim();
if (!triple) throw Error("Cannot determine Rust target");
const windowsRuntime = process.platform === "win32";
if (windowsRuntime && triple !== "x86_64-pc-windows-msvc")
  throw Error("The reviewed Windows desktop runtime requires x86_64-pc-windows-msvc");
// Package checks also reject compiler routing overrides. Keep build/dev from
// silently bypassing the fixed runtime flags or reading a stale target directory.
const runtimeOverrides = new Set([
  "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS",
  "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS", "CARGO_BUILD_TARGET",
  "CARGO_BUILD_TARGET_DIR", "CERULEAN_VERIFIER_SHA256", "CERULEAN_CHECKER_SHA256",
]);
if (windowsRuntime && Object.entries(process.env).some(([name, value]) =>
  value && runtimeOverrides.has(name.toUpperCase())))
  throw Error("Unset Cargo target-directory, target and rustflags overrides before building the desktop runtime");
// Build the offline tool first, then pin its exact bytes in both host binaries.
run("cargo", ["build", "-p", "cerulean-domain", "--bin", "cerulean-verifier", "--locked",
  ...(release ? ["--release"] : []),
  ...(windowsRuntime ? ["--target", triple, "--config", 'target.x86_64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]'] : []),
]);
const verifier = join(cargoTarget, ...(windowsRuntime ? [triple] : []), release ? "release" : "debug", `cerulean-verifier${windowsRuntime ? ".exe" : ""}`);
run("cargo", ["build", "-p", "cerulean-checker", "--locked",
  ...(release ? ["--release"] : []),
  ...(windowsRuntime ? ["--target", triple, "--config", 'target.x86_64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]'] : []),
]);
const checker = join(cargoTarget, ...(windowsRuntime ? [triple] : []), release ? "release" : "debug", `cerulean-checker${windowsRuntime ? ".exe" : ""}`);
const boundEnvironment = {...process.env, CERULEAN_VERIFIER_SHA256:createHash("sha256").update(readFileSync(verifier)).digest("hex"), CERULEAN_CHECKER_SHA256:createHash("sha256").update(readFileSync(checker)).digest("hex")};
run("cargo", [
  "build",
  "-p",
  "cerulean-core",
  "--bin",
  "cerulean-core",
  "--locked",
  ...(release ? ["--release"] : []),
  // Explicit --target confines crt-static to runtime target code and its C
  // dependencies; host build scripts/proc-macros and the later GUI keep their route.
  ...(windowsRuntime ? [
    "--target", triple,
    "--config", 'target.x86_64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]',
  ] : []),
], process.cwd(), boundEnvironment);
run("cargo", ["run", "-p", "cerulean-domain", "--bin", "export-contracts", "--locked"]);
const ext = process.platform === "win32" ? ".exe" : "";
const profile = release ? "release" : "debug";
const builtRuntime = windowsRuntime
  ? join(cargoTarget, triple, profile, `cerulean-core${ext}`)
  : join(cargoTarget, profile, `cerulean-core${ext}`);
// Preserve existing freezer/test/direct-launch paths, but stage only the output
// of the successful explicit-target build. Any missing output/copy error stops.
mkdirSync(join("target", profile), { recursive: true });
if (windowsRuntime)
  copyFileSync(builtRuntime, join("target", profile, `cerulean-core${ext}`));
// The desktop resolves its sibling runtime, including direct launches outside the CLI.
copyFileSync(builtRuntime, join("target", profile, `cerulean-runtime${ext}`));
mkdirSync(join(cargoTarget, profile), { recursive: true });
copyFileSync(builtRuntime, join(cargoTarget, profile, `cerulean-runtime${ext}`));
copyFileSync(verifier, join(cargoTarget, profile, `cerulean-verifier${ext}`));
mkdirSync("apps/desktop/src-tauri/binaries", { recursive: true });
copyFileSync(verifier, join("target", profile, `cerulean-verifier${ext}`));
copyFileSync(verifier, `apps/desktop/src-tauri/binaries/cerulean-verifier-${triple}${ext}`);
copyFileSync(checker, join(cargoTarget, profile, `cerulean-checker${ext}`));
copyFileSync(checker, join("target", profile, `cerulean-checker${ext}`));
copyFileSync(checker, `apps/desktop/src-tauri/binaries/cerulean-checker-${triple}${ext}`);
copyFileSync(
  builtRuntime,
  `apps/desktop/src-tauri/binaries/cerulean-runtime-${triple}${ext}`,
);
// Use the JS CLI entry rather than a shell-parsed command line.
run(
  process.execPath,
  [
    resolve("node_modules/@tauri-apps/cli/tauri.js"),
    release ? "build" : "dev",
    ...(mode === "build" ? ["--no-bundle"] : []),
    ...(mode === "package" ? ["--bundles", "nsis"] : []),
    ...(release ? ["--", "--locked"] : []),
  ],
  resolve("apps/desktop"),
  boundEnvironment,
);
if (release) {
  const gui = join(cargoTarget, profile, `CeruleanLonging${ext}`);
  copyFileSync(gui, join("target", profile, `CeruleanLonging${ext}`));
  const bundle = join(cargoTarget, profile, "bundle");
  if (existsSync(bundle)) cpSync(bundle, join("target", profile, "bundle"), { recursive: true });
}
