"""One-shot selective evidence snapshot for batch 064. Never overwrite archives."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p4-workbuddy-coding-preparation-20261011.json'
archive = reports / 'p4-workbuddy-coding-sources-20261011'
assert not index.exists() and not archive.exists(), 'Frozen evidence must not be rewritten'

def read(relative):
    return json.loads((root / relative).read_text(encoding='utf-8-sig'))

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

attempt = read('reports/workbuddy-coding-send-B3MM5u/result.json')
recovery = read('reports/workbuddy-coding-readback-RPcaX3/result.json')
parser = read('reports/workbuddy-rich-reply-htCVOh/result.json')
history = read('reports/workbuddy-history-4ZMEjE/parser-result.json')
send = read('reports/workbuddy-send-native-5Dn5Gt/result.json')
assert attempt['sendAttempts'] == 1 and attempt['status'] == 'failed'
assert attempt['artifact']['status'] == 'passed' and attempt['artifact']['checks'] == 15
for field in ['historyMatched', 'completed', 'requestedMarkerMatches', 'artifactStillMatches', 'displayedCodeMatchesArtifact']:
    assert recovery[field] is True, field
assert recovery['replyProven'] is False and recovery['reviewMarkerMatches'] is False
assert len(parser['cases']) == 32 and all(c['passed'] for c in parser['cases'])
assert parser['legacyPlainProofIdentical'] and parser['rustContractPassed']
assert history['status'] == send['status'] == 'passed' and len(history['cases']) == 19 and len(send['cases']) == 30
artifact = root / 'reports/workbuddy-coding-live-20261011-01/project/sum.mjs'
assert digest(artifact) == attempt['artifact']['sha256']
plan = root / 'tests/fixtures/workbuddy-coding-plan.json'
assert digest(plan) == attempt['planHash'] == '877800ca3589c7b4d3f69b56e253dfeced4c75534ba8487d46113aad3ec68e27'

owners = [
    'workbuddy-coding-preflight-owned-20261011', 'workbuddy-coding-live-owned-20261011',
    'workbuddy-coding-reconcile-owned-20261011', 'workbuddy-coding-structure-owned-20261011',
    'workbuddy-coding-readback-owned-20261011', 'workbuddy-coding-parser-owned-20261011',
    'workbuddy-coding-parser-final-owned-20261011', 'workbuddy-coding-recovery-final-owned-20261011',
    'workbuddy-coding-send-regression-owned-20261011', 'workbuddy-coding-parser-guard-owned-20261011',
    'workbuddy-coding-history-owned-20261011',
]
cleanup = {}
for owner in owners:
    item = read('reports/' + owner + '/cleanup.json')
    assert item['CleanupConfirmed'] and item['RemainingProcesses'] == 0 and not item['TimedOut']
    cleanup[owner] = {'exitCode': item['ExitCode'], 'remaining': item['RemainingProcesses']}

runtime_paths = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {p: digest(root / p) for p in runtime_paths}
assert len(set(runtimes.values())) == 1
for name in ['core', 'desktop']:
    assert 'Finished `dev`' in (reports / f'workbuddy-coding-{name}-final-build-20261011.log').read_text(encoding='utf-8-sig')

previous = reports / 'p4-workbuddy-rich-review-20261011.json'
prior = read(previous.relative_to(root))
for record in prior['records']:
    assert digest(root / record['snapshot']) == record['sha256'], record['snapshot']

source_files = [
    'AGENTS.md', 'docs/HANDOFF-LATEST.md', 'docs/PROTOTYPE-REUSE-BASELINE.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md', 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs',
    'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-coding-20261011.py',
    'tests/workbuddy-coding-live.mjs', 'tests/workbuddy-coding-readback.mjs', 'tests/workbuddy-coding-checks.mjs',
    'tests/workbuddy-send-reconcile.mjs', 'tests/workbuddy-rich-reply.mjs', 'tests/workbuddy-history.mjs',
    'tests/workbuddy-send-native.mjs', 'tests/fixtures/workbuddy-coding-plan.json',
    'tests/fixtures/workbuddy-coding-verifier.mjs',
]
folders = owners + [
    'workbuddy-coding-send-hrMK9H', 'workbuddy-coding-checks-WM858d', 'workbuddy-coding-send-B3MM5u',
    'workbuddy-send-reconcile-7EYp5h', 'desktop-capability-diagnostic-xLgkRN',
    'workbuddy-coding-readback-1cbHap', 'workbuddy-coding-readback-RPcaX3',
    'workbuddy-rich-reply-ZCKzzl', 'workbuddy-rich-reply-fBAyIN', 'workbuddy-rich-reply-htCVOh',
    'workbuddy-history-4ZMEjE', 'workbuddy-send-native-5Dn5Gt', 'workbuddy-coding-live-20261011-01',
]
files = {root / p for p in source_files}
for folder in folders:
    assert (reports / folder).is_dir(), folder
    files.update(p for p in (reports / folder).rglob('*') if p.is_file())
files.update(reports.glob('workbuddy-coding-*-build-20261011.log'))
files.add(reports / ('workbuddy-coding-send-' + attempt['planHash'] + '.json'))
archive.mkdir()
records = []
for source in sorted(files):
    relative = source.relative_to(root)
    snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, snapshot)
    checksum = digest(source)
    assert digest(snapshot) == checksum
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': checksum})

result = {
    'batch': '064', 'at': datetime.now().astimezone().isoformat(),
    'status': 'artifact-verified-read-only-recovered-original-task-proof-failed',
    'goalStatus': 'active', 'fullP6Accepted': False, 'liveSendAttempts': 1, 'resends': 0,
    'userApproved': True, 'planSha256': attempt['planHash'], 'artifactChecks': 15,
    'artifactSha256': attempt['artifact']['sha256'], 'originalAttempt': attempt,
    'readOnlyRecovery': recovery, 'parserCases': 32, 'historyCases': 19, 'controlledSendCases': 30,
    'legacyPlainProofIdentical': True, 'realCodingLayoutCaptured': True, 'boundedObservationMigrated': False,
    'durableWorkflowTested': False, 'managementTested': False, 'originalTaskProofPassed': False,
    'limitation': 'Approved prompt has 33 hex marker digits; internal review truncated to 32. No rewrite or resend. Read-only artifact and approved-text evidence does not upgrade original proof.',
    'archiveScope': 'Selective evidence and source snapshot, not a complete reproducible repository',
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous),
    'runtimeHashes': runtimes, 'cleanup': cleanup, 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(result, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': result['status']}, ensure_ascii=False))
