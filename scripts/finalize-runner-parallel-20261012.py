"""Freeze batch 085 after all changed-path checks; never rewrite earlier evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-runner-parallel-20261012.json'
archive = reports / 'p5-runner-parallel-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(stage):
    data = (reports / f'p5-runner-parallel-{stage}-20261012.log').read_bytes()
    return data.decode('utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous = reports / 'p5-api-parallel-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
assert len(prior['records']) == 64
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
ui_inherited = []
for record in ui['records']:
    if record['source'] in prior['inheritedUiSources']:
        assert checksum(root / record['source']) == record['sha256'], record['source']
        ui_inherited.append(record['source'])
assert len(ui_inherited) == 6
# The coordinator's assignment/budget transaction and original proof remain unchanged.
for name in ['crates/core/src/workflow_access.rs', 'crates/core/src/workflow_coordinator.rs',
             'crates/domain/src/execution/workspaces.rs', 'crates/core/src/api_startup.rs',
             'crates/core/src/api_execution/coordinator_budget_tests.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
def prior_source(name):
    record = next(r for r in prior['records'] if r['source'] == name)
    return (root / record['snapshot']).read_text(encoding='utf-8-sig')

# Scheduler changes only route the two rebuildable projection publications.
old_scheduler = prior_source('crates/core/src/scheduler.rs')
expected_scheduler = old_scheduler.replace('mod early_cancellation;',
    'mod early_cancellation;\n#[path = "scheduler_projection.rs"]\nmod projection;')
expected_scheduler = expected_scheduler.replace('fs::rename(temp, destination).map_err(|e| e.to_string())',
    'projection::publish(&temp, &destination)')
assert (root / 'crates/core/src/scheduler.rs').read_text(encoding='utf-8-sig').strip() == expected_scheduler.strip()
old_api = prior_source('crates/core/src/api_execution.rs')
expected_api = old_api.replace('    if result.is_err() {\n        unknown(store, task, id)?;\n    }',
    '    if let Err(original) = &result {\n        if let Err(recovery) = unknown(store, task, id) {\n            return Err(format!(\n                "{original}; recording unknown also failed: {recovery}"\n            ));\n        }\n    }')
assert (root / 'crates/core/src/api_execution.rs').read_text(encoding='utf-8-sig') == expected_api
assert '1 passed; 0 failed' in read('projection-unit2')
assert '1 passed; 0 failed' in read('projection-recovery2')
runner = read('runner2')
assert '3 passed; 0 failed' in runner
assert 'automatic parallel-success: two API workers from user run' in runner
assert 'automatic fail-parallel: two API workers from user run' in runner
assert 'resource wait: one worker and two management records while occupied' in runner
assert 'capacity refill: third ready task waits with zero reservation/billing' in runner
assert '9 passed; 0 failed' in read('git')
assert 'sealed reader: writer remains exclusive' in read('git')
assert '2 passed; 0 failed' in read('profiles')
assert '1 passed; 0 failed' in read('integration')
for mode in ['success', 'bad-whole', 'conflict']:
    assert f'integration scenario {mode}: passed' in read('integration')
full = read('full-check2')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 453, counts
assert sum(int(i) for _, i in counts) == 160, counts
assert '297 passed; 0 failed; 141 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '8 passed; 0 failed' in read('worktree')
assert '2 passed; 0 failed' in read('mixed')
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(stage)
cases = reports / 'p5-runner-parallel-cases2-20261012'
results = []
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    result = json.loads((cases / mode / 'result.json').read_text(encoding='utf-8-sig'))
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['finalReopenNewEvents'] == 0
    if mode != 'failure-after-cli':
        assert result['accepted'] == 3 and result['dispatchesPerTask'] == 1
        assert result['intermediateRestart'] and result['managementRequests'] == 7
        for name, expected in [('a.txt', 'A'), ('b.txt', 'B'), ('bundle.txt', 'AB')]:
            assert (Path(result['finalDirectory']) / name).read_text(encoding='utf-8') == expected
    else:
        assert result['accepted'] == 1 and result['finalOmpDispatches'] == 0
        assert result['resumeBlocked'] and result['partialWritePreserved'] and result['apiState'] == 'unknown'
    results.append(result)
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = {r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
names.update([
    'AGENTS.md',
    'crates/core/src/project_runner_parallel.rs', 'crates/core/src/resource_leases.rs',
    'crates/core/src/git_workspaces.rs', 'crates/core/src/git_workspaces/overlay.rs',
    'crates/core/src/git_workspaces/tests.rs',
    'crates/core/src/scheduler_projection.rs', 'crates/core/tests/scheduler_crash.rs',
    'crates/core/src/api_run_tests.rs', 'crates/core/src/integration_tests.rs', 'crates/core/src/integration_tests/profiles.rs',
    'crates/core/src/integration_tests/parallel.rs',
    'crates/core/src/api_assignment_scope.rs', 'crates/core/src/api_startup.rs',
    'crates/core/src/omp_host.rs', 'crates/core/src/desktop_background.rs',
    'scripts/check.ps1', 'scripts/check-runner-parallel-20261012.ps1',
    'scripts/finalize-runner-parallel-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-runner-parallel-*-20261012.log'))
paths.add(reports / 'p5-runner-parallel-wrapper-before-fix-20261012.ps1')
failed_case = reports / 'project-integration-Ke41mm'
paths.add(failed_case / 'failure-snapshot.json')
paths.update((failed_case / 'data/workflows/project/events').glob('*.json'))
paths.update((failed_case / 'data/workflows/project').glob('state.*'))
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
    'batch': '085', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'automatic-api-parallel-dispatch-with-resource-wait-and-readonly-workspace-inspection',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'runnerTests': 3, 'runnerScenarios': 4, 'gitTests': 9, 'worktreeTests': 8,
    'profileTests': 2, 'integrationScenarios': 3, 'mixedHostTests': 2,
    'defaultCorePassed': 297, 'defaultCoreIgnored': 141,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'user start_project_run automatically starts two real API threads; real Git, protected file tools, verifier subprocesses and chief acceptance; controlled manager/model responses',
    'publicApiParallelEnabled': True, 'automaticApiRunnerParallelEnabled': True,
    'sameProjectCliApiParallelEnabled': False, 'verificationWithActiveApiEnabled': False,
    'executionWorkerLimitGlobal': 2, 'newJournal': False,
    'verified': ['automatic independent API dispatch from original user run authority',
        'slot/dependency/resource waits without additional manager requests',
        'parallel completion then pause/reopen/resume independent verification and final integration',
        'one API failure retains unknown, other dispatched task can submit, final integration never starts',
        'shared resource serializes workers without treating contention as failure',
        'pool mutation remains exclusive; background writer wait cancels, times out or transfers ownership',
        'sealed ready inspection bypasses pool writer lock without write or repair authority',
        'third ready task waits with zero reservation/billing and refills one of two global slots',
        'only projection rename retries transient Windows sharing violation for up to one second; source and previous projection preserved',
        'projection failure retains committed intent; no redispatch, unknown recovery, second reopen adds zero events',
        'changed sealed record rejected; prior Git recovery, cancellation, rebinding and hook protections retained'],
    'mixedHostResults': results,
    'failuresPreserved': ['automatic simultaneous preparation failed because original read-only proof check held the pool writer lock; second attempt retained Unknown',
        'second test in initial run also hit leftover controlled gate after first failure',
        'diagnostic repeat identified workspace store already in use; both original failed logs retained',
        'first full regression exposed old fixture assuming Attention implies worker already returned the ledger; fixture now waits for actual owner completion before idle-ledger inspection',
        'third-task simultaneous completion exposed Windows sharing violation during state projection replacement; original fixture project-integration-Ke41mm retained; occupying process not identified',
        'first file-lock test held destination (Windows access-denied) rather than source (sharing-violation); corrected fixture separately covers both codes without broadening retry to access-denied',
        'first recovery test compilation used a private helper; removed that assertion and retained behavioral refusal/reopen checks',
        'full-check2 passed; PowerShell5 wrapper then treated existing Cargo stderr warning as terminating error before runner test; preserved wrapper and empty log, fixed native exit handling and resumed remaining stages only'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'projectionSharingObservation': {
        'failedFixture': failed_case.relative_to(root).as_posix(),
        'observedFailure': 'Windows sharing violation (32) while replacing state projection',
        'occupyingProcessIdentified': False,
        'mitigation': 'one-second retry window for projection rename only; all other errors stop',
        'modelOrCommandRetry': False,
    },
    'remaining': ['verification and chief-accept lifecycle with shared project ownership',
        'Codex/OMP shared ownership and full heterogeneous concurrent scheduling/lifecycle/budgets',
        'real heterogeneous coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
