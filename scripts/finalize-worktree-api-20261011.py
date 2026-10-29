"""Freeze batch 067 once. No real provider calls; preserve all earlier archives."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-api-20261011.json'
archive = reports / 'p5-worktree-api-sources-20261011'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


previous = reports / 'p5-worktree-binding-20261011.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']

for name, expected in [('native-02', '3 passed; 0 failed'), ('read', '1 passed; 0 failed'),
                       ('unknown', '1 passed; 0 failed'), ('core-regression', '275 passed; 0 failed; 95 ignored')]:
    assert expected in (reports / f'p5-worktree-api-{name}-20261011.log').read_text(encoding='utf-8-sig')
for name in ['clippy', 'core-build', 'desktop-build']:
    assert 'Finished `dev`' in (reports / f'p5-worktree-api-{name}-20261011.log').read_text(encoding='utf-8-sig')
assert (reports / 'p5-worktree-api-fmt-20261011.log').stat().st_size == 0

runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/core/src/api_host.rs', 'crates/core/src/api_execution.rs',
    'crates/core/src/omp_file_tools.rs', 'crates/core/src/omp_file_tools/api.rs',
    'crates/core/src/application_recovery.rs', 'crates/core/src/codex_management_bridge.rs',
    'crates/core/src/api_host_tests.rs', 'crates/core/src/api_host_worktree_tests.rs',
    'crates/core/src/git_workspaces/validation_fixture.rs',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'scripts/finalize-worktree-api-20261011.py',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-worktree-api-*-20261011.log'))
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
    'batch': '067', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'api-worktree-host-checks-passed', 'fullP5Accepted': False, 'fullP6Accepted': False,
    'nativeCases': 5, 'syntheticProviderRoutes': 8, 'defaultCorePassed': 275, 'defaultCoreIgnored': 95,
    'realModelCalls': 0, 'desktopActions': 0,
    'apiHostDispatchIntegrated': True, 'cliWorktreeDispatchIntegrated': False,
    'asynchronousWorkspacePreparation': False, 'configurationUiEnabled': False,
    'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'receiptScope': 'synthetic API and management replies; real Git, file tools, durable host and independent validator',
    'recoveryScope': 'lost ACK and missing-ready fixtures with host reopen; not process crash or power loss',
    'runtimeHashes': runtimes, 'previousIndex': previous.relative_to(root).as_posix(),
    'previousIndexSha256': checksum(previous), 'archiveScope': 'selective source/evidence snapshot',
    'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
