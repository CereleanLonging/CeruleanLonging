"""Freeze batch 068 once; selective evidence archive, no real provider calls."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-async-20261011.json'
archive = reports / 'p5-worktree-async-sources-20261011'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


previous = reports / 'p5-worktree-api-20261011.json'
for record in json.loads(previous.read_text(encoding='utf-8-sig'))['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name, expected in [('native-02', '3 passed; 0 failed'), ('api-regression', '5 passed; 0 failed'),
                       ('materialization-cancel', '1 passed; 0 failed')]:
    assert expected in (reports / f'p5-worktree-async-{name}-20261011.log').read_text(encoding='utf-8-sig')
for name in ['clippy', 'clippy-final', 'desktop-build']:
    assert 'Finished `dev`' in (reports / f'p5-worktree-async-{name}-20261011.log').read_text(encoding='utf-8-sig')
full = (reports / 'p5-worktree-async-full-check-20261011.log').read_text(encoding='utf-8-sig')
results = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', full)
assert results and all(failed == '0' for _, failed, _ in results)
assert sum(int(p) for p, _, _ in results) == 430
assert sum(map(int, re.findall(r'ℹ pass (\d+)', full))) == 5
assert len(re.findall(r'ℹ fail 0', full)) == 2

runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/core/src/api_host.rs', 'crates/core/src/async_api.rs', 'crates/core/src/api_startup.rs',
    'crates/core/src/api_publication.rs', 'crates/core/src/host.rs', 'crates/core/src/scheduler.rs',
    'crates/core/src/git_workspaces.rs', 'crates/core/src/git_workspaces/tests.rs',
    'crates/core/src/diagnostic_service.rs', 'crates/core/src/api_host_worktree_tests.rs',
    'crates/core/src/api_workspace_async_tests.rs', 'apps/desktop/src/ApiPublicationReview.tsx',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'scripts/finalize-worktree-async-20261011.py',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-worktree-async-*-20261011.log'))
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
    'batch': '068', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'owned-api-workspace-preparation-checks-passed', 'fullP5Accepted': False, 'fullP6Accepted': False,
    'newHostNativeCases': 3, 'newRealMaterializationCancellationCases': 1,
    'regressionNativeCases': 5, 'syntheticProviderRoutes': 8,
    'defaultRustPassed': 430, 'defaultRustIgnoredAtFullCheck': 117, 'nodePassed': 5,
    'supplementalCheck': 'one native cancellation test added and run after full check; final core all-targets clippy passed',
    'realModelCalls': 0, 'desktopActions': 0, 'asynchronousApiWorkspacePreparation': True,
    'cliWorktreeDispatchIntegrated': False, 'configurationUiEnabled': False,
    'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'responsivenessScope': 'host queried/cancelled while owned preparation thread paused; later synchronous Git identity gates and full-load UI latency not certified',
    'recoveryScope': 'simulated ACK/partial-publication windows, host reopen, and actual file-materialization cancellation; not power loss',
    'runtimeHashes': runtimes, 'previousIndex': previous.relative_to(root).as_posix(),
    'previousIndexSha256': checksum(previous), 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
