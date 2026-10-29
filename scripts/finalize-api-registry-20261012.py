"""Freeze batch 083 once; preserve the original journals and prior evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-api-registry-20261012.json'
archive = reports / 'p5-api-registry-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-dispatch-budget-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
assert len(prior['records']) == 46
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
assert '4 passed; 0 failed; 8 ignored' in read('p5-api-registry-targeted2-20261012.log')
shared = read('p5-api-registry-shared2-20261012.log')
assert '1 passed; 0 failed' in shared
assert 'cancelFirst=false: two worker threads, one writer, two dispatches' in shared
assert 'cancelFirst=true: two worker threads, one writer, two dispatches' in shared
assert shared.count('stale progress ignored, reopen events 0') == 2
budget_sources = ['crates/core/src/api_execution.rs', 'crates/core/src/workflow_access.rs',
    'crates/core/src/api_execution/coordinator_budget_tests.rs', 'crates/core/src/scheduler.rs',
    'crates/domain/src/execution/workspaces.rs']
prior_by_source = {record['source']: record for record in prior['records']}
for name in budget_sources:
    assert checksum(root / name) == prior_by_source[name]['sha256'], name
writer_source = 'crates/core/src/workflow_coordinator.rs'
old_writer = (root / prior_by_source[writer_source]['snapshot']).read_text(encoding='utf-8')
expected_writer = old_writer.replace('    #[cfg(test)]\n    Inspect(', '    Inspect(', 1).replace(
    '                    #[cfg(test)] Message::Inspect(', '                    Message::Inspect(', 1).replace(
    '    #[cfg(test)]\n    pub(crate) fn snapshot(', '    pub(crate) fn snapshot(', 1)
assert (root / writer_source).read_text(encoding='utf-8') == expected_writer

full = read('p5-api-registry-full-check-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 450, counts
assert sum(int(i) for _, i in counts) == 154, counts
assert '295 passed; 0 failed; 135 ignored' in full
assert 'approved_worktree_reservations_allow_independent_tasks_but_retain_attempt_ownership ... ok' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '8 passed; 0 failed' in read('p5-api-registry-worktree-20261012.log')
assert '2 passed; 0 failed' in read('p5-api-registry-mixed-20261012.log')
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-api-registry-{stage}-20261012.log')
cases = reports / 'p5-api-registry-cases-20261012'
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
    'crates/domain/src/execution/workspaces.rs', 'crates/core/src/project_planning.rs',
    'crates/core/src/execution_resource_tests.rs', 'crates/core/src/host.rs',
    'crates/core/src/workflow_access.rs', 'crates/core/src/workflow_coordinator.rs',
    'crates/core/src/workflow_coordinator_tests.rs', 'crates/core/src/scheduler.rs',
    'crates/core/src/api_execution.rs', 'crates/core/src/api_execution/tests.rs',
    'crates/core/src/api_execution/coordinator_budget_tests.rs', 'crates/core/src/managed_assignment.rs',
    'crates/core/src/async_verification.rs', 'crates/core/src/async_api.rs',
    'crates/core/src/api_host.rs', 'crates/core/src/api_host_tests.rs',
    'crates/core/src/api_registry_tests.rs', 'crates/core/src/codex_management_bridge.rs',
    'crates/core/src/api_controls.rs', 'crates/core/src/api_controls_tests.rs',
    'crates/core/src/api_publication_tests.rs', 'crates/core/src/application_recovery.rs',
    'crates/core/src/project_runner.rs', 'crates/core/src/unsent_assignment.rs',
    'crates/core/src/api_host_worktree_tests.rs', 'crates/core/src/api_workspace_async_tests.rs',
    'crates/core/src/integration_tests/mixed.rs',
    'scripts/check.ps1', 'scripts/check-api-registry-20261012.ps1',
    'scripts/finalize-api-registry-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-api-registry-*-20261012.log'))
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
    'batch': '083', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'project-owned-api-writers-and-attempt-keyed-activity',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'sharedRegistryTests': 1, 'sharedRegistryScenarios': 2, 'worktreeTests': 8,
    'defaultCorePassed': 295, 'defaultCoreIgnored': 135,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'actual Git and two production API worker threads sharing one writer; controlled second startup enters through durable worker protocols below public admission; normal public second start is asserted rejected; synthetic provider and manager replies only',
    'parallelDispatchEnabled': False, 'publicActiveAttemptLimitPerProject': 1, 'internalConcurrentApiWorkersVerified': 2, 'newJournal': False,
    'verified': ['project-owned writer registry', 'project/attempt cancellation and completion lookup', 'last worker completion transfers the writer', 'first completion or cancellation preserves the other active worker', 'one authoritative project snapshot ignores stale worker progress', 'two dispatches total and no restart events', 'existing eight API routes, cancellation, startup and heterogeneous serial integration'],
    'inheritedBudgetSources': budget_sources,
    'mixedHostResults': results,
    'failuresPreserved': ['ExecutionSnapshot lacks PartialEq; used original stable digest for same-sequence check', 'initial shared lifecycle fixture called public management_context and hit the original serial guard; used existing snapshot context helper inside controlled fixture, without weakening production admission'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'budgetRule': 'original full max_rounds reservation until sealed completion; a partial allowance may conservatively account more than remaining capacity; no unknown release or automatic retry',
    'remaining': ['parallel management context and assignment acknowledgement proofs, startup sequence gap validation', 'public API admission using shared writer, default model concurrency 2 globally and at most 2 per project', 'Codex/OMP and verification ownership migration, heterogeneous concurrent budgets', 'independent parallel dispatch in runner', 'real heterogeneous coding and efficiency including failures', 'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
