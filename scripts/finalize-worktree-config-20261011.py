"""Freeze batch 071 once, including failed attempts and final local evidence."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-config-20261011.json'
archive = reports / 'p5-worktree-config-sources-20261011'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-worktree-codex-20261011.json'
for record in json.loads(previous.read_text(encoding='utf-8-sig'))['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
assert '2 passed; 0 failed' in read('p5-worktree-config-native-complete-20261011.log')
checks = read('p5-worktree-config-full-check-final-20261011.log')
assert 'test result: FAILED' not in checks
rust = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', checks)
assert rust and '275 passed; 0 failed; 111 ignored' in checks
assert 'pass 2' in read('p5-worktree-config-ipc-final-20261011.log')
assert 'fail 0' in read('p5-worktree-config-ipc-final-20261011.log')
assert 'pass 3' in read('p5-worktree-config-service-final-20261011.log')
assert 'fail 0' in read('p5-worktree-config-service-final-20261011.log')
assert 'built in' in read('p5-worktree-config-frontend-final-20261011.log')
for name in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-worktree-config-{name}-20261011.log')
ui = reports / 'worktree-project-form-l49hOJ'
result = json.loads((ui / 'result.json').read_text(encoding='utf-8'))
assert result['status'] == 'passed' and len(result['cases']) == 6
cleanup = json.loads(read('worktree-config-ui-owned-03-20261011/cleanup.json'))
assert cleanup['CleanupConfirmed'] and cleanup['ExitCode'] == 0 and cleanup['RemainingProcesses'] == 0
runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/project_setup.rs', 'crates/domain/src/bin/export-contracts.rs',
    'crates/core/src/project_setup.rs', 'crates/core/src/project_planning.rs',
    'crates/core/src/worktree_configuration.rs', 'crates/core/src/host.rs',
    'crates/core/src/codex_cancel_native.rs',
    'apps/desktop/src/ProjectPlanning.tsx', 'apps/desktop/src/WorktreeFields.tsx',
    'apps/desktop/src/contracts.ts', 'tests/worktree-project-form.mjs',
    'scripts/test-omp-stop-review-native.ps1', 'scripts/check.ps1',
    'scripts/finalize-worktree-config-20261011.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-worktree-config-*-20261011.log'))
for directory in reports.glob('worktree-config-ui-owned*20261011'):
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
    'batch': '071', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'worktree-project-configuration-checks-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'newNativeCases': 2, 'uiCases': result['cases'],
    'rustDefaultPassed': sum(int(p) for p, _ in rust),
    'rustDefaultIgnored': sum(int(i) for _, i in rust), 'nodeChecksPassed': 5,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'configurationUiEnabled': True, 'saveCreatesWorktrees': False,
    'parallelDispatchEnabled': False, 'dependencyArtifactTransferEnabled': False,
    'integrationTaskImplemented': False,
    'scope': 'Real host configuration/persistence/approval with synthetic chief replies and real pinned Git file; production React form with recording sink, not native Tauri IPC',
    'failureHistory': [
        'Initial compile fixture errors corrected; initial native dot-path case normalized itself on Windows, replaced with literal alias input',
        'Concurrent cargo compile hit locked test executable; final checks run sequentially',
        'Sandbox blocked browser/process/named-pipe checks; owned local checks rerun with escalation',
        'UI saved-configuration fixture used draft acceptance shape; corrected to legacy fallback, production form unchanged',
        'Generated Rust contract allows null; form adapter now normalizes null to undefined. Final frontend, IPC and builds rerun after correction; passed Rust suite retained.',
    ],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'runtimeHashes': runtimes, 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed']}, ensure_ascii=False))
