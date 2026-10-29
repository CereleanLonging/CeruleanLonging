"""Freeze batch 074 once: resident dependent DAG evidence, offline transports."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-dependency-hosts-20261012.json'
archive = reports / 'p5-dependency-hosts-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-dependency-overlay-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name in ['api-final', 'api-gates', 'omp', 'omp-legacy', 'codex-success', 'codex-recover-a', 'codex-legacy']:
    assert '1 passed; 0 failed' in read(f'p5-dependency-host-{name}-20261012.log')
checks = read('p5-dependency-host-core-regression-20261012.log')
match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', checks)
assert match and int(match[1]) == 278
assert 'Finished `dev`' in read('p5-dependency-host-clippy-20261012.log')
assert 'Finished `dev`' in read('p5-dependency-host-fixtures-20261012.log')
assert not read('p5-dependency-host-fmt-20261012.log').strip()
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert runtimes == prior['runtimeHashes'], 'Tests-only batch must identify the unchanged runtime evidence'
peers = {name: checksum(root / name) for name in [
    'target/debug/examples/codex_dag_fixture.exe', 'target/debug/examples/omp_offline_worker_fixture.exe',
]}
codex = {}
for mode in ['success', 'recover-a', 'legacy']:
    directory = reports / f'p5-dependency-host-codex-{mode}-20261012'
    value = json.loads((directory / 'result.json').read_text(encoding='utf-8'))
    assert value['status'] == 'passed' and value['turnStarts'] == 2 and value['finalRestartNewEvents'] == 0
    assert value['bDispatchBeforeAAccept'] == 0
    if mode == 'recover-a':
        assert value['explicitRecoveryPausedActiveRun']
    codex[mode] = value
names = [
    'crates/core/src/host.rs', 'crates/core/src/dependency_host_fixture.rs',
    'crates/core/src/api_run_tests.rs', 'crates/core/src/omp_project_tests.rs',
    'crates/core/src/project_dag_tests.rs', 'crates/core/examples/codex_dag_fixture.rs',
    'crates/core/examples/omp_offline_worker_fixture.rs',
    'scripts/test-dependency-hosts-20261012.ps1', 'scripts/finalize-dependency-hosts-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-dependency-host-*-20261012.log'))
for mode in codex:
    directory = reports / f'p5-dependency-host-codex-{mode}-20261012'
    paths.update(directory / name for name in ['result.json', 'snapshot.json', 'details.json', 'bin/protocol.jsonl'])
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
    'batch': '074', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'resident-dependency-worktree-dag-checks-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'apiRoutes': 8, 'apiFailureGateScenarios': 4,
    'ompScenarios': 4, 'ompLegacyScenarios': 4,
    'codexScenarios': codex,
    'coreDefaultPassed': int(match[1]), 'coreDefaultIgnored': int(match[2]),
    'realModelCalls': 0, 'userDesktopActions': 0,
    'functionalProductionChanges': False,
    'scope': 'Existing resident API/OMP/Codex scheduling, real Git/file tools/verification/reopen; synthetic manager/API and local offline CLI peers',
    'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'heterogeneousWorkersInOneProjectVerified': False,
    'runtimeEvidenceReusedBatch': '073',
    'firstFailure': 'API test temporary parent retained ../ segments. Canonicalized fixture path; existing product rejection unchanged. No provider request was sent.',
    'remaining': ['explicit independent project integration and final acceptance', 'multiple configured workers in one project and parallel resource leases', 'real heterogeneous efficiency', 'all original P0-P6 acceptance requirements'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'runtimeHashes': runtimes, 'offlineFixtureHashes': peers,
    'archiveScope': 'selective source/evidence snapshot; no credentials or full data directories', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
