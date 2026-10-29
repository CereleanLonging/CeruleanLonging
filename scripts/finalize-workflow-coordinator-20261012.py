"""Freeze batch 081 once; preserve the original journals and prior evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-workflow-coordinator-20261012.json'
archive = reports / 'p5-workflow-coordinator-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-execution-resources-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
assert len(prior['records']) == 41
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
ui_index = reports / 'p5-resource-config-20261012.json'
ui = json.loads(ui_index.read_text(encoding='utf-8-sig'))
ui_inherited = []
for record in ui['records']:
    if record['source'] in prior['inheritedUiSources']:
        assert checksum(root / record['source']) == record['sha256'], record['source']
        ui_inherited.append(record['source'])
assert len(ui_inherited) == 6
assert '5 passed; 0 failed' in read('p5-workflow-coordinator-targeted3-20261012.log')
assert '1 passed; 0 failed' in read('p5-workflow-coordinator-routes-20261012.log')
full = read('p5-workflow-coordinator-full-check-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 450, counts
assert sum(int(i) for _, i in counts) == 148, counts
assert '295 passed; 0 failed; 129 ignored' in full
assert 'api_waiting_for_provider_does_not_hold_the_journal_writer ... ok' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '8 passed; 0 failed' in read('p5-workflow-coordinator-worktree-20261012.log')
assert '2 passed; 0 failed' in read('p5-workflow-coordinator-mixed-20261012.log')
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-workflow-coordinator-{stage}-20261012.log')
cases = reports / 'p5-workflow-coordinator-cases-20261012'
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
    'crates/core/src/lib.rs', 'crates/core/src/host.rs',
    'crates/core/src/workflow_access.rs', 'crates/core/src/workflow_coordinator.rs',
    'crates/core/src/workflow_coordinator_tests.rs', 'crates/core/src/scheduler.rs',
    'crates/core/src/api_execution.rs', 'crates/core/src/managed_assignment.rs',
    'crates/core/src/async_verification.rs', 'crates/core/src/async_api.rs',
    'crates/core/src/api_host.rs', 'crates/core/src/api_host_tests.rs',
    'crates/core/src/api_host_worktree_tests.rs', 'crates/core/src/api_workspace_async_tests.rs',
    'crates/core/src/integration_tests/mixed.rs',
    'scripts/check.ps1', 'scripts/check-workflow-coordinator-20261012.ps1',
    'scripts/finalize-workflow-coordinator-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-workflow-coordinator-*-20261012.log'))
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    paths.update(cases / mode / name for name in ['result.json', 'snapshot.json', 'bin/protocol.jsonl'])
assert all(p.is_file() for p in paths), [str(p) for p in paths if not p.is_file()]
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
    'batch': '081', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'resident-api-worker-uses-scoped-single-writer-coordinator',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'targetedTests': 5, 'apiRouteBranches': 8, 'worktreeTests': 8,
    'defaultCorePassed': 295, 'defaultCoreIgnored': 129,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'production API journal coordinator, actual local journals/Git/files/CLI/validators; synthetic provider and management replies; no user desktop sends',
    'parallelDispatchEnabled': False, 'activeAttemptLimitPerProject': 1, 'newJournal': False,
    'verified': ['scoped revision/task/attempt commands', 'one original event for concurrent duplicate operation', 'lost acknowledgement forbids retry', 'publication interruption reopens original event', 'writer responds during blocked provider and preparation', 'original two durable assignment acknowledgements', 'existing cancellation/worktree/recovery and heterogeneous integration'],
    'mixedHostResults': results,
    'failuresPreserved': ['new test missing Entry import and unwrap_err Debug requirement corrected', 'duplicate-operation fixture wrongly expected two successful require_new commands; original require_new semantics preserved and flag threaded explicitly; final five tests passed'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'remaining': ['project-shared coordinator registry and per-attempt active state', 'approved worktree domain concurrency policy', 'atomic concurrent project request reservations', 'Codex/OMP and verification ownership migration', 'independent parallel dispatch and attempt-isolated cancellation/Unknown', 'real heterogeneous coding and efficiency including failures', 'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
