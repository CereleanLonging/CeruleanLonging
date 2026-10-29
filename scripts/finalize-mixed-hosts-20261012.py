"""Freeze batch 077 once; keep earlier source snapshots and user credentials untouched."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-mixed-hosts-20261012.json'
archive = reports / 'p5-mixed-hosts-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')

previous = reports / 'p5-worker-profiles-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
inherited_sources = []
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
    source = Path(record['source'])
    if source.parts[0] in ['crates', 'apps', 'tests'] and source.as_posix() != 'crates/core/src/integration_tests.rs':
        assert checksum(root / source) == record['sha256'], source
        inherited_sources.append(source.as_posix())
for name in ['native', 'failure', 'omp-legacy', 'codex-legacy']:
    assert '1 passed; 0 failed' in read(f'p5-mixed-hosts-{name}-20261012.log')
assert '278 passed; 0 failed; 129 ignored' in read('p5-mixed-hosts-core-20261012.log')
assert 'Finished `dev`' in read('p5-mixed-hosts-clippy-20261012.log')
assert not read('p5-mixed-hosts-fmt-20261012.log').strip()
cases = reports / 'p5-mixed-hosts-cases-20261012'
results = []
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    result = json.loads((cases / mode / 'result.json').read_text(encoding='utf-8-sig'))
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['finalReopenNewEvents'] == 0
    if mode != 'failure-after-cli':
        assert result['accepted'] == 3 and result['dispatchesPerTask'] == 1
        assert result['intermediateRestart'] and result['managementRequests'] == 7
        final = Path(result['finalDirectory'])
        for filename, expected in [('a.txt', 'A'), ('b.txt', 'B'), ('bundle.txt', 'AB')]:
            assert (final / filename).read_text() == expected
    else:
        assert result['accepted'] == 1 and result['finalOmpDispatches'] == 0
        assert result['resumeBlocked'] and result['partialWritePreserved'] and result['apiState'] == 'unknown'
    results.append(result)
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert runtimes == prior['runtimeHashes']
names = [
    'crates/core/src/host.rs', 'crates/core/src/integration_tests.rs',
    'crates/core/src/integration_tests/mixed.rs', 'crates/core/src/dependency_host_fixture.rs',
    'crates/core/src/project_dag_tests.rs', 'crates/core/src/omp_project_tests.rs',
    'crates/core/examples/codex_dag_fixture.rs', 'crates/core/examples/omp_offline_worker_fixture.rs',
    'scripts/check-mixed-hosts-20261012.ps1', 'scripts/finalize-mixed-hosts-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-mixed-hosts-*-20261012.log'))
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    paths.update(cases / mode / name for name in ['result.json', 'snapshot.json', 'bin/protocol.jsonl'])
legacy = reports / 'p5-mixed-hosts-legacy-codex-20261012'
paths.update(p for p in legacy.glob('*.json') if p.is_file())
paths.add(legacy / 'bin/protocol.jsonl')
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
    'batch': '077', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'same-project-codex-omp-api-final-integration-and-failure-gate-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False, 'productionChanges': False,
    'sameProjectRoutes': ['codex', 'omp', 'deepseek API'],
    'finalIntegratorRoutesVerified': ['deepseek API', 'codex', 'omp'],
    'executionScope': 'real Git, resident host routes, offline CLI processes, host file tools and independent validators; synthetic model replies',
    'cases': results, 'defaultCorePassed': 278, 'defaultCoreIgnored': 129,
    'legacyRegression': ['OMP default/external profile completion/cancellation (four variants)', 'Codex original two-task DAG with independent checks and restart'],
    'inheritedChecks': '076 frontend, domain, IPC/service, product builds and 17 UI cases; unchanged production and runtime hashes',
    'unchangedPriorSources': inherited_sources, 'runtimeHashes': runtimes,
    'realModelCalls': 0, 'userDesktopActions': 0, 'parallelDispatchEnabled': False,
    'mixedDesktopProfilesLiveVerified': False, 'realHeterogeneousEfficiencyVerified': False,
    'unresolvedObservation': prior['unresolvedObservation'],
    'remaining': ['per-attempt/directory/channel leases and parallel dispatch', 'real heterogeneous coding and efficiency including failures', 'Git lifecycle reliability observation', 'all original first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective synthetic reports and source; excludes vaults and full data directories; temporary OMP fixture runtimes removed after checks; reproduce in fresh cases',
    'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'cases': len(results), 'runtimeUnchanged': True}, ensure_ascii=False))
