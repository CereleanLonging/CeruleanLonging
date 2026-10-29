"""Freeze 086 only after the changed-path acceptance checks pass."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-shared-verification-20261012.json'
archive = reports / 'p5-shared-verification-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(stage):
    raw = (reports / f'p5-shared-verification-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous = reports / 'p5-runner-parallel-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
assert len(prior['records']) == 107
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
ui_inherited = []
for record in ui['records']:
    if record['source'] in prior['inheritedUiSources']:
        assert checksum(root / record['source']) == record['sha256'], record['source']
        ui_inherited.append(record['source'])
assert len(ui_inherited) == 6
for name in ['crates/core/src/workflow_access.rs', 'crates/core/src/api_execution.rs',
             'crates/core/src/api_startup.rs', 'crates/core/src/api_execution/coordinator_budget_tests.rs',
             'crates/core/src/git_workspaces.rs', 'crates/domain/src/execution/workspaces.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
record = next(r for r in prior['records'] if r['source'] == 'crates/core/src/workflow_coordinator.rs')
old = (root / record['snapshot']).read_text(encoding='utf-8-sig')
current = (root / record['source']).read_text(encoding='utf-8-sig')
for start, end in [('impl Scope {', '#[derive(Clone)]\nstruct Cursor'),
                   ('                    Message::Apply {scope,cursor', '                    #[cfg(test)] Message::InterruptNextPublication')]:
    assert old[old.index(start):old.index(end)] == current[current.index(start):current.index(end)]

full = read('full2')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 453, counts
assert sum(int(i) for _, i in counts) == 161, counts
assert '297 passed; 0 failed; 142 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '3 passed; 0 failed' in read('runner')
assert '1 passed; 0 failed' in read('lifecycle3')
for mode in ['last-api-exits', 'refill-during-verify', 'cancel-verifier', 'lose-verify-ack', 'changed-artifact']:
    assert f'shared verification {mode}: one writer, one verifier process' in read('lifecycle3')
for stage, count in [('worktree', 8), ('budget', 6), ('mixed', 2)]:
    assert f'{count} passed; 0 failed' in read(stage)
for stage in ['clippy', 'desktop']:
    assert 'Finished `dev`' in read(stage)
cases = reports / 'p5-shared-verification-cases-20261012'
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
    'AGENTS.md', 'crates/core/src/scheduler_verification.rs',
    'crates/core/src/coordinated_verification.rs', 'crates/core/src/shared_verification_host.rs',
    'crates/core/src/verification_detail.rs', 'crates/core/src/management_history.rs',
    'crates/core/src/verifier.rs', 'crates/core/src/verifier/process.rs',
    'crates/core/src/source_snapshot.rs', 'crates/core/src/verification_recovery.rs',
    'crates/core/src/integration_tests/shared_verification.rs',
    'tests/fixtures/shared-verification-check.cjs',
    'scripts/check.ps1', 'scripts/check-shared-verification-20261012.ps1',
    'scripts/finalize-shared-verification-20261012.py',
    'scripts/finalize-shared-verification-docs-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/NEXT-STAGE-GATES.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-shared-verification-*-20261012.log'))
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
    'batch': '086', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'independent-verification-shares-api-writer-and-lifecycle',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'runnerTests': 3, 'runnerScenarios': 4, 'sharedVerificationTests': 1, 'sharedVerificationScenarios': 5,
    'worktreeTests': 8, 'budgetTests': 6, 'mixedHostTests': 2,
    'defaultCorePassed': 297, 'defaultCoreIgnored': 142,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'specializedCheckSeconds': {
        stage: float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', read(stage))[-1])
        for stage in ['runner', 'lifecycle3', 'worktree', 'budget', 'mixed']
    },
    'finalLogs': {
        stage: f'reports/p5-shared-verification-{stage}-20261012.log'
        for stage in ['full2', 'runner', 'lifecycle3', 'worktree', 'budget', 'mixed', 'clippy', 'desktop']
    },
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'real Git, API threads, pinned Node verifier subprocess, file tools and journal; controlled model and management replies',
    'automaticApiRunnerParallelEnabled': True, 'verificationWithActiveApiEnabled': True,
    'sameProjectCliApiParallelEnabled': False, 'chiefAcceptanceWithActiveApiEnabled': False,
    'executionWorkerLimitGlobal': 2, 'backgroundOperationLimitGlobal': 4, 'verificationLimitPerProject': 1,
    'newJournal': False, 'apiWorkerVerificationAuthority': False,
    'verified': ['original authority captured under writer; verifier runs outside writer; same target evidence revalidated on commit',
        'verification client is consumed once; original verifier and immutable evidence remain authoritative',
        'A verification while B executes, and C dispatch while A verifier and B API remain active',
        'last API completion retains writer until verifier exits',
        'verification cancellation preserves other API attempt; failure blocks downstream without repeating checks',
        'lost verification ACK retains published Verify and reopens from original record without reexecution',
        'changed source artifact rejects verification, retains failure and blocks downstream',
        'running and completed detail reads from original history do not poll or mutate',
        'API client cannot forge Verify; duplicate verifier and premature writer transfer rejected'],
    'mixedHostResults': results,
    'failuresPreserved': ['initial compilation used private ProjectRun import, corrected to original public domain type',
        'initial fixture custom environment rejected by original allowlist; changed fixture to pinned Node script and approved args; environment whitelist unchanged',
        'final lifecycle detail check exposed pending Unknown with committed Verify being rejected by old terminal-status equality; read now permits pending status plus original recorded evidence without reconciliation, contradictory terminal state remains rejected; initial passing full/runner logs retained'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'projectionSharingObservation': prior['projectionSharingObservation'],
    'remaining': ['chief acceptance with shared writer authority',
        'Codex/OMP shared startup/ACK/cancel/recovery and full heterogeneous concurrent scheduling',
        'real heterogeneous coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
