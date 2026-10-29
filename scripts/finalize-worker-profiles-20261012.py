"""Freeze batch 076 once: per-task named execution configurations and local evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worker-profiles-20261012.json'
archive = reports / 'p5-worker-profiles-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')

previous = reports / 'p5-final-integration-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
native = read('p5-worker-profiles-host-postfix-20261012.log')
assert '2 passed; 0 failed' in native
for mode in ['same-adapter', 'mixed-adapter']:
    assert f'named profiles {mode}: 3 accepted tasks, 10 synthetic API rounds, final AB, reopen events 0' in native
assert '1 passed; 0 failed' in read('p5-worker-profiles-resume-fix-20261012.log')
full = read('p5-worker-profiles-full-check-final-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert counts and 'test result: FAILED' not in full
assert sum(int(p) for p, _ in counts) == 433
assert sum(int(i) for _, i in counts) == 146
assert 'pass 2' in full and 'pass 3' in full and 'built in' in full
for name in ['clippy-final', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-worker-profiles-{name}-20261012.log')
ui_runner = reports / 'p5-worker-profiles-ui-owned-final-20261012'
ui_value = json.loads((ui_runner / 'stdout.log').read_text(encoding='utf-8-sig').strip().splitlines()[-1])
assert ui_value['status'] == 'passed' and len(ui_value['cases']) == 17
ui = Path(ui_value['artifact'])
for runner, code in [(ui_runner, 0), (reports / 'p5-worker-profiles-ui-owned-20261012', 1)]:
    cleanup = json.loads((runner / 'cleanup.json').read_text(encoding='utf-8-sig'))
    assert cleanup['CleanupConfirmed'] and cleanup['RemainingProcesses'] == 0 and cleanup['ExitCode'] == code
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/execution.rs', 'crates/domain/src/management.rs',
    'crates/domain/src/project_setup.rs', 'crates/domain/src/project_setup/profiles.rs',
    'crates/domain/src/bin/export-contracts.rs', 'crates/adapters/src/openai.rs',
    'crates/core/src/project_planning.rs', 'crates/core/src/project_setup.rs',
    'crates/core/src/project_runner.rs', 'crates/core/src/desktop_management.rs',
    'crates/core/src/management_execution.rs', 'crates/core/src/worktree_configuration.rs',
    'crates/core/src/host.rs', 'crates/core/src/supervision_requests.rs',
    'crates/core/src/integration_tests.rs', 'crates/core/src/integration_tests/profiles.rs',
    'apps/desktop/src/WorkerProfiles.tsx', 'apps/desktop/src/ApiWorkerFields.tsx',
    'apps/desktop/src/DesktopWorkerFields.tsx', 'apps/desktop/src/ProjectPlanning.tsx',
    'apps/desktop/src/WorkflowPlans.tsx', 'apps/desktop/src/contracts.ts',
    'tests/worktree-project-form.mjs', 'scripts/check.ps1',
    'scripts/finalize-worker-profiles-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
# Include legacy fixture constructors whose new optional fields are explicitly empty.
for path in (root / 'crates').rglob('*.rs'):
    source = path.read_text(encoding='utf-8-sig')
    if 'profile_id:' in source or 'profiles:' in source:
        paths.add(path)
paths.update(reports.glob('p5-worker-profiles-*-20261012.log'))
for runner in [ui_runner, reports / 'p5-worker-profiles-ui-owned-20261012']:
    paths.update(runner / name for name in ['stdout.log', 'stderr.log', 'cleanup.json'])
paths.update(ui / name for name in ['result.json', 'entry.tsx', 'worktree-config.png', 'integration-config.png', 'worker-profile.png'])
assert all(p.is_file() for p in paths)
archive.mkdir()
records = []
for path in sorted(paths):
    relative = path.relative_to(root)
    snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, snapshot)
    value = checksum(path)
    assert checksum(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {
    'batch': '076', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'per-task-named-execution-profiles-local-checks-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'namedProfilesImplemented': True, 'sameProjectNamedApiProfilesVerified': True,
    'verifiedScenarios': ['two DeepSeek model configurations plus primary final integration', 'DeepSeek and Grok configurations plus primary final integration'],
    'executionScope': 'actual resident API/Git/files/validators; synthetic management and provider responses',
    'approvedProfileBinds': ['adapter identity', 'model', 'pinned executable or API/desktop selection', 'artifacts', 'acceptance checks'],
    'failureGates': ['unknown profile', 'selected provider credential missing', 'same-adapter model substitution after approval', 'rehashed persisted provider/adapter mismatch'],
    'finalRestartNewEvents': 0, 'uiScenarios': 17,
    'rustDefaultPassed': sum(int(p) for p, _ in counts),
    'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'realModelCalls': 0, 'userDesktopActions': 0,
    'parallelDispatchEnabled': False, 'mixedCliApiProfilesVerified': False,
    'mixedDesktopProfilesLiveVerified': False, 'realHeterogeneousEfficiencyVerified': False,
    'unresolvedObservation': prior['unresolvedObservation'],
    'failuresPreserved': [
        'initial constructor replacement also matched SavedProjectSetup; two compiler errors fixed',
        'new nested test module path corrected',
        'initial UI fixture lacked crypto.randomUUID; non-secret configuration IDs now use first unused local ID',
        'full regression exposed pause/resume ledger ownership error; readiness reads existing host snapshots including active worker ledgers; focused supervision regression and final full checks passed',
    ],
    'remaining': ['mixed CLI/API final integration', 'per-attempt/channel resource leases and parallel scheduling', 'real heterogeneous coding and efficiency including failure costs', 'Git lifecycle reliability observation', 'full original P0-P6 requirements including real management, first-batch access, desktop reliability, install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
