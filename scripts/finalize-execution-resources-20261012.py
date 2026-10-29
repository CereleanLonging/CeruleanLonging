"""Freeze batch 080 once, including reused mixed-host regression evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-execution-resources-20261012.json'
archive = reports / 'p5-execution-resources-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')

previous = reports / 'p5-resource-config-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
ui_inherited = []
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
    if record['source'].startswith('apps/desktop/src/') or record['source'] == 'tests/worktree-project-form.mjs':
        assert checksum(root / record['source']) == record['sha256']
        ui_inherited.append(record['source'])
assert '9 passed; 0 failed' in read('p5-execution-resources-targeted-final-20261012.log')
full = read('p5-execution-resources-full-check-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 445, counts
assert sum(int(i) for _, i in counts) == 148, counts
assert '290 passed; 0 failed; 129 ignored' in full
assert 'desktop_locks_follow_window_and_provider_session_instead_of_binding_labels ... ok' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '2 passed; 0 failed' in read('p5-execution-resources-mixed-20261012.log')
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-execution-resources-{stage}-20261012.log')
cases = reports / 'p5-execution-resources-cases-20261012'
results = []
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    result = json.loads((cases / mode / 'result.json').read_text())
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['finalReopenNewEvents'] == 0
    if mode != 'failure-after-cli':
        assert result['accepted'] == 3 and result['dispatchesPerTask'] == 1
        assert result['intermediateRestart'] and result['managementRequests'] == 7
        for name, expected in [('a.txt', 'A'), ('b.txt', 'B'), ('bundle.txt', 'AB')]:
            assert (Path(result['finalDirectory']) / name).read_text() == expected
    else:
        assert result['accepted'] == 1 and result['finalOmpDispatches'] == 0
        assert result['resumeBlocked'] and result['partialWritePreserved'] and result['apiState'] == 'unknown'
    results.append(result)
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = [
    'crates/core/src/host.rs', 'crates/core/src/resource_leases.rs',
    'crates/core/src/execution_resources.rs', 'crates/core/src/execution_resource_tests.rs',
    'crates/core/src/artifacts.rs', 'crates/core/src/codex_management_bridge.rs',
    'crates/core/src/omp_host.rs', 'crates/core/src/api_host.rs',
    'crates/core/src/desktop_management.rs', 'crates/core/src/management_application.rs',
    'crates/core/src/integration_tests.rs', 'crates/core/src/integration_tests/mixed.rs',
    'crates/core/examples/codex_dag_fixture.rs', 'crates/core/examples/omp_offline_worker_fixture.rs',
    'scripts/check.ps1', 'scripts/check-execution-resources-20261012.ps1',
    'scripts/finalize-execution-resources-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-execution-resources-*-20261012.log'))
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    paths.update(cases / mode / name for name in ['result.json', 'snapshot.json', 'bin/protocol.jsonl'])
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
    'batch': '080', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'implicit-directory-and-desktop-channel-resources-gated-and-replayed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'resourceTypes': ['explicit case-sensitive name', 'canonical directory read/write overlap', 'desktop process/window', 'Qoder provider chat UUID'],
    'targetedTests': 9, 'newTargetedTests': 4, 'defaultCorePassed': 290, 'defaultCoreIgnored': 129,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'real journals/replay; resource-only worktree configuration uses non-executed git.exe fixture; desktop identity unit cases are controlled data; reused mixed-host cases use actual Git, CLI children and verification with synthetic model/manager replies',
    'parallelDispatchEnabled': False, 'newLeaseJournal': False,
    'mixedHostResults': results,
    'failuresPreserved': ['resource-only configuration fixture used test executable name, rejected by existing git.exe check; corrected fixture path without weakening production', 'freeze preflight referenced restartNewEvents instead of original finalReopenNewEvents; corrected against original 077 schema, no index/archive existed after failed preflight'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'remaining': ['per-attempt activity and single-writer coordination', 'parallel dispatch and unknown resource ownership', 'real heterogeneous coding and efficiency including failures', 'all original first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
