"""Freeze batch 069 once. Offline peers, no model or desktop calls."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-omp-20261011.json'
archive = reports / 'p5-worktree-omp-sources-20261011'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


previous = reports / 'p5-worktree-async-20261011.json'
for record in json.loads(previous.read_text(encoding='utf-8-sig'))['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name, result in [('native-final', '4 passed; 0 failed'), ('legacy-native', '1 passed; 0 failed')]:
    assert result in (reports / f'p5-worktree-omp-{name}-20261011.log').read_text(encoding='utf-8-sig')
for name in ['clippy', 'core-build', 'desktop-build']:
    assert 'Finished `dev`' in (reports / f'p5-worktree-omp-{name}-20261011.log').read_text(encoding='utf-8-sig')
regression = (reports / 'p5-worktree-omp-core-regression-20261011.log').read_text(encoding='utf-8-sig')
match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', regression)
assert match
assert not (reports / 'p5-worktree-omp-fmt-20261011.log').read_text(encoding='utf-8-sig').strip()
runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/core/src/async_omp.rs', 'crates/core/src/omp_execution.rs',
    'crates/core/src/omp_file_tools.rs', 'crates/core/src/omp_host.rs',
    'crates/core/src/omp_management_bridge.rs', 'crates/core/src/omp_management_bridge/tests.rs',
    'crates/core/src/omp_host/tests.rs', 'crates/core/src/omp_host/tests/worktree.rs',
    'crates/core/src/host.rs', 'crates/core/src/git_workspaces/validation_fixture.rs',
    'crates/core/examples/omp_offline_worker_fixture.rs',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'scripts/finalize-worktree-omp-20261011.py',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-worktree-omp-*-20261011.log'))
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
    'batch': '069', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'omp-worktree-host-checks-passed', 'fullP5Accepted': False, 'fullP6Accepted': False,
    'newNativeCases': 4, 'legacyResidentNativeCases': 1,
    'newManagedOutcomes': ['submitted-and-verified-accepted', 'cancelled-after-write', 'unknown-after-write', 'rejected-model-identity'],
    'coreDefaultPassed': int(match[1]), 'coreDefaultIgnored': int(match[2]),
    'realModelCalls': 0, 'desktopActions': 0,
    'workerPeer': 'owned local offline executable using synthetic credential only',
    'ompWorktreeDispatchIntegrated': True, 'codexWorktreeDispatchIntegrated': False,
    'processCwd': 'isolated OMP profile; approved files exposed through host RPC tools',
    'configurationUiEnabled': False, 'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'recoveryScope': 'simulated prep pause/ACK loss/ready removal, actual process terminal loss and host reopen; not power loss',
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'runtimeHashes': runtimes, 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
