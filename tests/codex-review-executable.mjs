import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream, mkdtempSync, readFileSync, readdirSync, statSync, truncateSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { coreLibraryFromMessages } from './cargo-core-artifact.mjs';

assert.equal(process.platform, 'win32');
assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const deps = resolve(process.argv[2] || join(process.env.CARGO_TARGET_DIR, 'debug/deps'));
const profile = dirname(deps);
// The full suite can leave both test-feature and production libraries here.
// Bind to the exact successful build stream when the caller supplies one.
const cores = process.argv[3]
  ? [coreLibraryFromMessages(readFileSync(process.argv[3], 'utf8'), deps)]
  : readdirSync(deps).filter(file => file.startsWith('libcerulean_core-') && file.endsWith('.rlib'));
assert.equal(cores.length, 1, 'Unambiguous core build required');
const coreIdentity = cores[0].split('-').at(-1).replace('.rlib', '');
const coreFingerprint = readFileSync(join(profile, '.fingerprint', `cerulean-core-${coreIdentity}`, 'lib-cerulean_core.json'), 'utf8');
function library(name) {
  let matches = readdirSync(deps).filter(file => file.startsWith(`lib${name}-`) && file.endsWith('.rlib'));
  // Tests and production can enable different features of the same dependency.
  // Select the exact fingerprint linked by the core, preserving its u64 precision.
  const expected = coreFingerprint.match(new RegExp(`\\[\\d+,\\s*"${name}",\\s*false,\\s*(\\d+)\\]`))?.[1];
  assert(expected, `Core dependency fingerprint required for ${name}`);
  matches = matches.filter(file => {
    const identity = file.split('-').at(-1).replace('.rlib', '');
    const folders = readdirSync(join(profile, '.fingerprint')).filter(folder => folder.endsWith(`-${identity}`));
    assert.equal(folders.length, 1);
    const bytes = readFileSync(join(profile, '.fingerprint', folders[0], `lib-${name}`), 'utf8').trim();
    return Buffer.from(bytes, 'hex').readBigUInt64LE() === BigInt(expected);
  });
  assert.equal(matches.length, 1, `Unambiguous ${name} build required`);
  return join(deps, matches[0]);
}
const source = readFileSync('crates/core/src/async_codex.rs', 'utf8');
const host = readFileSync('crates/core/src/host.rs', 'utf8');
const review = readFileSync('crates/core/src/local_review.rs', 'utf8');
assert.equal(review.match(/async_codex::describe_executable/g)?.length, 3);
assert(!review.includes('PinnedCheck::describe'));
assert(source.includes('executable: describe_executable(executable)?'));
const functions = source.slice(source.indexOf('fn inspect_executable('), source.indexOf('pub(crate) fn describe('));
const ordinary = host.slice(host.indexOf('pub(crate) fn ordinary('), host.indexOf('pub(crate) fn config('));
assert(functions.includes('fn describe_executable(') && ordinary.includes('reparse host path'));
const temporary = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'codex-review-size-'));
const harness = join(temporary, 'pin.rs');
writeFileSync(harness, `
use std::{fs::{self, File, OpenOptions}, io::Read, path::Path};
use cerulean_domain::execution::{require, ValidationFile};
${ordinary}
${functions}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let file = Path::new(&args[1]);
    match args[2].as_str() {
        "hash" => println!("{}", describe_executable(file).unwrap().sha256),
        "oversize" => assert_eq!(describe_executable(file).unwrap_err(), "Codex executable too large"),
        "relative" => assert!(describe_executable(Path::new("codex.exe")).is_err()),
        "lock" => {
            let (guard, _) = inspect_executable(file.to_str().unwrap()).unwrap();
            assert!(OpenOptions::new().write(true).open(file).is_err());
            drop(guard);
            assert!(OpenOptions::new().write(true).open(file).is_ok());
        },
        _ => panic!("unknown case"),
    }
}
`);
function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8', windowsHide: true, timeout: 90000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  return result.stdout.trim();
}
const executable = join(temporary, 'pin.exe');
// Explicit-target builds keep proc-macro DLLs in the host profile directory.
// Rust must resolve both directories when loading the production domain crate.
const hostDeps = join(process.env.CARGO_TARGET_DIR, basename(dirname(deps)), 'deps');
run('rustc', ['--edition=2021', '-C', 'target-feature=+crt-static',
  '-L', `dependency=${deps}`, '-L', `dependency=${hostDeps}`,
  '--extern', `sha2=${library('sha2')}`, '--extern', `cerulean_domain=${library('cerulean_domain')}`,
  harness, '-o', executable]);
async function hash(file) {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(file)) digest.update(chunk);
  return digest.digest('hex');
}
const large = join(temporary, 'codex.exe');
writeFileSync(large, 'synthetic bounded executable hashing fixture');
truncateSync(large, 256 * 1024 * 1024 + 1);
assert.equal(run(executable, [large, 'hash']), await hash(large));
run(executable, [large, 'lock']);
run(executable, [large, 'relative']);
truncateSync(large, 512 * 1024 * 1024 + 1);
run(executable, [large, 'oversize']);
let actual;
if (process.env.CERULEAN_REVIEW_CODEX_PROBE) {
  const file = process.env.CERULEAN_REVIEW_CODEX_PROBE;
  const bytes = statSync(file).size;
  assert(bytes > 256 * 1024 * 1024 && bytes <= 512 * 1024 * 1024);
  const expected = await hash(file);
  assert.equal(run(executable, [file, 'hash']), expected);
  actual = { bytes, sha256: expected };
}
console.log(JSON.stringify({ passed: true, cases: 4, actual, modelCalls: 0, executableLaunches: 0 }));
