"""Freeze batch 073 once. Local DAG composition/preparation, no real models."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-dependency-overlay-20261012.json'
archive = reports / 'p5-dependency-overlay-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-accepted-inputs-20261012.json'
for record in json.loads(previous.read_text(encoding='utf-8-sig'))['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name, count in [('approved-final', 9), ('config', 3), ('dispatch-guard-final', 1), ('merge', 3)]:
    assert f'{count} passed; 0 failed' in read(f'p5-dependency-overlay-{name}-20261012.log')
checks = read('p5-dependency-overlay-full-check-20261012.log')
rust = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', checks)
assert rust and 'test result: FAILED' not in checks
assert 'pass 2' in checks and 'pass 3' in checks and 'built in' in checks
for name in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-dependency-overlay-{name}-20261012.log')
ui = reports / 'worktree-project-form-BCuycv'
result = json.loads((ui / 'result.json').read_text(encoding='utf-8'))
assert result['status'] == 'passed' and len(result['cases']) == 8
cleanup = json.loads(read('dependency-overlay-ui-final-owned-20261012/cleanup.json'))
assert cleanup['CleanupConfirmed'] and cleanup['ExitCode'] == 0 and cleanup['RemainingProcesses'] == 0
runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/project_setup.rs', 'crates/domain/src/execution/workspaces.rs',
    'crates/core/src/project_setup.rs', 'crates/core/src/project_planning.rs',
    'crates/core/src/worktree_configuration.rs', 'crates/core/src/scheduler.rs',
    'crates/core/src/accepted_inputs.rs', 'crates/core/src/git_workspaces.rs',
    'crates/core/src/git_workspaces/merge.rs', 'crates/core/src/git_workspaces/overlay.rs',
    'crates/core/src/git_workspaces/tests/approved.rs',
    'crates/core/src/git_workspaces/tests/approved/overlay.rs',
    'apps/desktop/src/ProjectPlanning.tsx', 'apps/desktop/src/WorktreeFields.tsx',
    'apps/desktop/src/WorkflowPlans.tsx', 'apps/desktop/src/contracts.ts',
    'tests/worktree-project-form.mjs', 'scripts/finalize-dependency-overlay-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-dependency-overlay-*-20261012.log'))
for directory in reports.glob('dependency-overlay-ui-*20261012'):
    paths.update(path for path in directory.iterdir() if path.is_file())
paths.update(ui / name for name in ['result.json', 'worktree-config.png', 'entry.tsx'])
assert all(path.is_file() for path in paths)
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
    'batch': '073', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'approved-dependency-worktree-preparation-checks-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'newNativeOverlayCases': 3, 'nativeConfigurationCases': 3,
    'approvedWorkspaceRegressionCases': 6, 'mergeCases': 3,
    'dispatchGuardFinalCase': 1, 'uiCases': result['cases'],
    'rustDefaultPassed': sum(int(p) for p, _ in rust),
    'rustDefaultIgnored': sum(int(i) for _, i in rust), 'nodeChecksPassed': 5,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'dependentWorkspacePreparationEnabled': True,
    'explicitDependencyApprovalRequired': True, 'dependencyOverlayPersisted': True,
    'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'scope': 'Trusted DurableWorkflow real Git preparation, actual file writes and independent verifier with synthetic receipts; React recording sink, not native Tauri IPC',
    'recoveryScope': 'Actual partial-copy cancellation; simulated ready loss and host reopen; no power-loss claim',
    'remaining': ['API/OMP/Codex resident dependent-DAG integration evidence', 'independent integration/final verification', 'parallel leases and real heterogeneous efficiency', 'full original P0-P6 scope'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'runtimeHashes': runtimes, 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed']}, ensure_ascii=False))
