"""Freeze batch 079 once; preserve previous evidence and include final checks."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-resource-config-20261012.json'
archive = reports / 'p5-resource-config-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')

previous = reports / 'p5-resource-leases-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
assert '3 passed; 0 failed' in read('p5-resource-config-targeted-final-20261012.log')
full = read('p5-resource-config-full-check-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 441, counts
assert sum(int(i) for _, i in counts) == 148, counts
assert '286 passed; 0 failed; 129 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-resource-config-{stage}-20261012.log')
owned = reports / 'p5-resource-config-ui-owned-20261012'
cleanup = json.loads((owned / 'cleanup.json').read_text(encoding='utf-8-sig'))
assert cleanup['ExitCode'] == 0 and cleanup['CleanupConfirmed'] and cleanup['RemainingProcesses'] == 0
ui = json.loads((owned / 'stdout.log').read_text(encoding='utf-8-sig').strip().splitlines()[-1])
assert ui['status'] == 'passed' and len(ui['cases']) == 23 and ui['modelCalls'] == 0
ui_dir = Path(ui['artifact'])
assert ui_dir.resolve().is_relative_to(reports.resolve())
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/project_setup.rs', 'crates/domain/src/project_setup/profiles.rs',
    'crates/core/src/host.rs', 'crates/core/src/project_setup.rs',
    'crates/core/src/project_planning.rs', 'crates/core/src/project_runner.rs',
    'crates/core/src/management_execution.rs', 'crates/core/src/resource_config_tests.rs',
    'crates/core/src/resource_leases.rs', 'crates/core/src/codex_cancel_native.rs',
    'crates/core/src/integration_tests/profiles.rs', 'crates/core/src/integration_tests/mixed.rs',
    'apps/desktop/src/ProjectPlanning.tsx', 'apps/desktop/src/WorkerProfiles.tsx',
    'apps/desktop/src/ResourceFields.tsx', 'apps/desktop/src/WorkflowPlans.tsx',
    'apps/desktop/src/contracts.ts', 'tests/worktree-project-form.mjs',
    'scripts/check.ps1', 'scripts/test-omp-stop-review-native.ps1', 'scripts/test-codex-cancel-live.ps1',
    'scripts/finalize-resource-config-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
    'reports/p5-resource-config-work-notes-20261012.txt',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-resource-config-*-20261012.log'))
paths.update(p for p in owned.iterdir() if p.is_file())
paths.update(p for p in ui_dir.iterdir() if p.suffix in ['.json', '.png', '.tsx'])
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
    'batch': '079', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'configured-resources-bound-to-task-approval-and-replay-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'scope': 'project common resources plus named profile extras, deterministic task union, management application and run scope binding, UI declaration and approval display',
    'targetedTests': 3, 'uiCases': 23, 'defaultCorePassed': 286, 'defaultCoreIgnored': 129,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes,
    'realModelCalls': 0, 'userDesktopActions': 0, 'ownedBrowserRemainingProcesses': 0,
    'executionScope': 'real configuration persistence and plan journals with controlled manager reply; interactive production React form recording sink, not native IPC or live model test',
    'parallelDispatchEnabled': False, 'resourceConfigurationUiImplemented': True,
    'generatedPlansDeclareResources': True, 'newLeaseJournal': False,
    'failuresPreserved': ['helper default GBK read failed before edits, first filtered log had zero tests', 'test-only missing imports and wrong StartupJournal accessor', 'fixture request identifier missing ui-plan- prefix left old plan unchanged; corrected fixture and asserted revision increment'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'remaining': ['directory/channel resource strategy', 'per-attempt activity and single-writer coordination', 'parallel dispatch and unknown resource ownership', 'real heterogeneous coding and efficiency including failures', 'all original first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
