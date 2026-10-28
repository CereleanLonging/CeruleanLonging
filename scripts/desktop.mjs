import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
const mode = process.argv[2];
if (!["dev", "build", "package"].includes(mode)) throw Error("Expected dev, build or package");
const release = mode !== "dev";
const run = (bin, args, cwd = process.cwd()) => {
  const r = spawnSync(bin, args, { stdio: "inherit", shell: false, cwd });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
};
if (mode === "package") {
  run("python", ["scripts/sqlite-notices.py", "--check"]);
  run("python", ["scripts/distribution-notices.py", "--check"]);
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
  "CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR",
]);
if (windowsRuntime && Object.entries(process.env).some(([name, value]) =>
  value && runtimeOverrides.has(name.toUpperCase())))
  throw Error("Unset Cargo target-directory, target and rustflags overrides before building the desktop runtime");
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
]);
run("cargo", ["run", "-p", "cerulean-domain", "--bin", "export-contracts", "--locked"]);
const ext = process.platform === "win32" ? ".exe" : "";
const profile = release ? "release" : "debug";
const builtRuntime = windowsRuntime
  ? join("target", triple, profile, `cerulean-core${ext}`)
  : join("target", profile, `cerulean-core${ext}`);
// Preserve existing freezer/test/direct-launch paths, but stage only the output
// of the successful explicit-target build. Any missing output/copy error stops.
mkdirSync(join("target", profile), { recursive: true });
if (windowsRuntime)
  copyFileSync(builtRuntime, join("target", profile, `cerulean-core${ext}`));
// The desktop resolves its sibling runtime, including direct launches outside the CLI.
copyFileSync(builtRuntime, join("target", profile, `cerulean-runtime${ext}`));
mkdirSync("apps/desktop/src-tauri/binaries", { recursive: true });
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
);
