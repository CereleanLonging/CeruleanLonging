import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

assert.equal(process.platform, 'win32');
assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const source = readFileSync('crates/core/src/desktop_sessions.rs', 'utf8').replace(/\r\n/g, '\n');
const routing = source.slice(source.indexOf('    let sending = matches!('), source.indexOf('    require(\n        !outcome.timed_out'));
assert(routing.includes('crate::verifier::process::run_desktop'));
assert(routing.includes('crate::verifier::process::run\n'));
const names = ['ReviewPrepare', 'ReviewSend', 'ReviewRead', 'Discover', 'Inspect', 'TrialPrepare', 'TrialSend',
  'TaskPrepare', 'TaskSend', 'TaskStopPrepare', 'TaskStop', 'TaskStopRead', 'TaskRead', 'TaskProgress', 'TrialRead'];
const temporary = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'desktop-send-routing-'));
const harness = join(temporary, 'routing.rs');
writeFileSync(harness, `
use std::{collections::BTreeMap, time::Duration};
enum ReadRequest { ${names.map(name => `${name} {}`).join(', ')} }
mod verifier { pub mod process {
    use super::super::*;
    pub fn run(_: &str, _: &[String], _: &str, _: &str, timeout: Duration,
        _: &bool, _: &BTreeMap<String, String>) -> Result<(bool, u64), String> {
        Ok((false, timeout.as_secs()))
    }
    pub fn run_desktop(_: &str, _: &[String], _: &str, _: &str, timeout: Duration,
        _: &bool, _: &BTreeMap<String, String>) -> Result<(bool, u64), String> {
        Ok((true, timeout.as_secs()))
    }
} }
fn check(request: &ReadRequest) -> Result<(bool, u64), String> {
    let executable = "bridge";
    let working = "work";
    let text = String::new();
    let cancel = &false;
    ${routing}
    Ok(outcome)
}
fn main() {
    ${names.map(name => {
      const sends = ['ReviewSend', 'TrialSend', 'TaskSend'].includes(name);
      return `assert_eq!(check(&ReadRequest::${name} {}).unwrap(), (${sends || ['TaskStop', 'ReviewPrepare'].includes(name)}, ${sends ? 90 : 20}), "${name}");`;
    }).join('\n    ')}
}
`);
const executable = join(temporary, 'routing.exe');
for (const [program, args] of [['rustc', ['--edition=2021', harness, '-o', executable]], [executable, []]]) {
  const result = spawnSync(program, args, { encoding: 'utf8', windowsHide: true, timeout: 60000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
}
console.log(JSON.stringify({ passed: true, cases: names.length, nativeUiActions: 0, modelCalls: 0 }));
