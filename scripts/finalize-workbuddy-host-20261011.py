"""Freeze the bounded host integration delta; never rerun an existing checkpoint."""
from pathlib import Path
import hashlib
import json
import shutil
from datetime import datetime, timezone, timedelta

root = Path(__file__).resolve().parents[1]
report = root / 'reports/p4-workbuddy-host-delta-20261011.json'
archive = root / 'reports/p4-workbuddy-host-sources-20261011'
assert not report.exists() and not archive.exists(), 'Checkpoint already exists'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

checks = {
    'workbuddy-host-delta-20261011.log': '12 passed; 0 failed; 5 ignored',
    'workbuddy-host-final-20261011.log': '12 passed; 0 failed; 2 ignored',
    'workbuddy-host-version-guard-20261011.log': '1 passed; 0 failed; 0 ignored',
    'workbuddy-host-clippy-20261011.log': 'Finished `dev` profile',
    'workbuddy-host-core-build-20261011.log': 'Finished `dev` profile',
    'workbuddy-host-desktop-build-20261011.log': 'Finished `dev` profile',
    'workbuddy-host-frontend-final-20261011.log': 'built in',
}
for name, expected in checks.items():
    raw = (root / 'reports' / name).read_bytes()
    log = raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
    assert expected in log, name
runtime = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
           'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime_hashes = {p: sha(root / p) for p in runtime}
assert len(set(runtime_hashes.values())) == 1
sources = [
    'AGENTS.md', 'docs/PROTOTYPE-REUSE-BASELINE.md', 'docs/HANDOFF-LATEST.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'crates/core/src/desktop_execution.rs', 'crates/core/src/desktop_management.rs',
    'crates/core/src/desktop_sessions_tests.rs', 'crates/core/src/desktop_run_tests.rs',
    'apps/desktop/src/DesktopTasks.tsx', 'apps/desktop/src/DesktopWorkerFields.tsx',
    'scripts/finalize-workbuddy-host-20261011.py',
    'tests/workbuddy-restart-review.mjs', 'tests/workbuddy-restart-send.mjs',
    'tests/fixtures/workbuddy-restart-plan.json', 'scripts/test-omp-stop-review-native.ps1',
    'reports/workbuddy-restart-review-LetSeS/result.json',
    'reports/workbuddy-restart-focus-owned-20261011/stdout.log',
    'reports/workbuddy-restart-focus-owned-20261011/cleanup.json',
    'reports/workbuddy-host-frontend-20261011.log',
] + ['reports/' + name for name in checks]
prototype = [
    'multi-agent-workbench-implementation-plan-v2.md',
    'workbench-probes/reports/prototype-technical-acceptance-2026-10-07.json',
    'workbench-probes/scratch/workbuddy-acceptance-repeat-20261007/review.json',
    'workbench-probes/reports/qoder-glm-comparison-2026-10-07.json',
    'workbench-probes/reports/workbuddy-cancellation-review-2026-10-07.json',
    'workbench-probes/STEP-25-WORKBUDDY-CANCEL-RECOVERY.md',
    'workbench-probes/src/workbuddy-targeted.ts',
    'workbench-probes/native/DesktopBridge.cs',
]
inputs = [(root / p, p) for p in sources] + [(root.parent / p, 'prototype/' + p) for p in prototype]
assert all(p.is_file() for p, _ in inputs), 'Missing source evidence'
archive.mkdir()
records = []
for source, name in inputs:
    destination = archive / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    assert sha(source) == sha(destination)
    records.append({'source': str(source), 'snapshot': str(destination.relative_to(root)), 'sha256': sha(destination)})
body = {
    'batch': '062', 'at': datetime.now(timezone(timedelta(hours=8))).isoformat(),
    'status': 'product-host-integration-tested-with-synthetic-transports',
    'goalStatus': 'active', 'fullP6Accepted': False,
    'inheritedPrototypeAcceptance': 'WorkBuddy 20/20 and cancellation/recovery; Qoder selected GLM 20/20 remain passed in recorded scope',
    'newHostTests': 5, 'testBatchesOverlap': True,
    'checks': checks, 'liveMessagesSent': 0,
    'testScope': 'Real persistent host, artifacts and Node validation; synthetic manager and desktop responses. No new real application acceptance.',
    'runtimeHashes': runtime_hashes,
    'previousIndex': 'reports/p4-workbuddy-click-review-20261011.json',
    'previousIndexSha256': sha(root / 'reports/p4-workbuddy-click-review-20261011.json'),
    'remaining': ['rich coding/tool replies', 'current real WorkBuddy product coding/cancel acceptance', 'real OpenAI management', 'all first-release integrations', 'P5 worktree integration', 'P6 full release acceptance'],
    'records': records,
}
with report.open('x', encoding='utf-8') as out:
    json.dump(body, out, ensure_ascii=False, indent=2)
    out.write('\n')
print(json.dumps({'report': str(report), 'records': len(records), 'status': body['status']}, ensure_ascii=False))
