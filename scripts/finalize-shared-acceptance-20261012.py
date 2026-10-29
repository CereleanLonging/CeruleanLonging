"""Freeze 087 with the original authority and failure evidence retained."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-shared-acceptance-20261012.json'
archive = reports / 'p5-shared-acceptance-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'
def checksum(path):
    return sha256(path.read_bytes()).hexdigest()
def read(stage):
    raw = (reports / f'p5-shared-acceptance-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous = reports / 'p5-shared-verification-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
assert len(prior['records']) == 90
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name in ['crates/core/src/workflow_access.rs', 'crates/core/src/api_execution.rs',
             'crates/core/src/api_startup.rs', 'crates/core/src/api_execution/coordinator_budget_tests.rs',
             'crates/core/src/git_workspaces.rs', 'crates/domain/src/execution/workspaces.rs',
             'crates/core/src/verifier.rs', 'crates/core/src/verifier/process.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
for name in prior['inheritedUiSources']:
    record = next(r for r in ui['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
record = next(r for r in prior['records'] if r['source'] == 'crates/core/src/workflow_coordinator.rs')
old = (root / record['snapshot']).read_text(encoding='utf-8-sig')
current = (root / record['source']).read_text(encoding='utf-8-sig')
for start, end in [('impl Scope {', '#[derive(Clone)]\nstruct Cursor'),
                   ('                    Message::Apply {scope,cursor', '                    #[cfg(test)] Message::InterruptNextPublication')]:
    assert old[old.index(start):old.index(end)] == current[current.index(start):current.index(end)]
record = next(r for r in prior['records'] if r['source'] == 'crates/core/src/api_assignment_scope.rs')
old = (root / record['snapshot']).read_text(encoding='utf-8-sig')
function = old[old.index('pub(super) fn prefix('):old.index('/// Validate only')]
new = (root / 'crates/core/src/workflow_prefix.rs').read_text(encoding='utf-8')
assert function.replace('pub(super)', 'pub(crate)', 1).strip() == new[new.index('pub(crate) fn prefix('):].strip()

full = read('full')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 453, counts
assert sum(int(i) for _, i in counts) == 163, counts
assert '297 passed; 0 failed; 144 ignored' in full
assert '297 passed; 0 failed; 144 ignored' in read('core-final')
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '4 passed; 0 failed' in read('runner')
assert 'capacity refill dependent=true' in read('runner')
assert '1 passed; 0 failed' in read('lifecycle4')
for mode in ['interleaved', 'lost-ack', 'lost-ack-reopen']:
    assert f'shared chief {mode}: one Accept' in read('lifecycle4')
assert 'shared chief missing-event: zero Accept' in read('lifecycle4')
assert '1 passed; 0 failed' in read('verification')
assert '2 passed; 0 failed' in read('mixed')
for stage in ['clippy-final', 'core-build-final', 'desktop-final']:
    assert 'Finished `dev`' in read(stage)
cases = reports / 'p5-shared-acceptance-cases-20261012'
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
    'AGENTS.md', 'crates/core/src/lib.rs', 'crates/core/src/management_application.rs',
    'crates/core/src/management_execution.rs', 'crates/core/src/acceptance_scope.rs',
    'crates/core/src/shared_acceptance_host.rs', 'crates/core/src/coordinated_acceptance.rs',
    'crates/core/src/workflow_prefix.rs', 'crates/core/src/integration_tests/shared_acceptance.rs',
    'scripts/check.ps1', 'scripts/check-shared-acceptance-20261012.ps1',
    'scripts/check-shared-acceptance-final-20261012.ps1',
    'scripts/finalize-shared-acceptance-20261012.py',
    'scripts/finalize-shared-acceptance-docs-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/NEXT-STAGE-GATES.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-shared-acceptance-*-20261012.log'))
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
    'batch': '087', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'chief-acceptance-shares-writer-and-unlocks-dependent-api',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'runnerTests': 4, 'runnerScenarios': 5, 'chiefApplicationTests': 1, 'chiefApplicationScenarios': 4,
    'sharedVerificationTests': 1, 'sharedVerificationScenarios': 5, 'mixedHostTests': 2,
    'defaultCorePassed': 297, 'defaultCoreIgnored': 144,
    'rustDefaultPassed': 453, 'rustDefaultIgnored': 163, 'nodeTests': 5,
    'runtimeHashes': runtimes, 'inheritedUiSources': prior['inheritedUiSources'],
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'real Git, API threads, original verifier processes, file tools and journal; controlled model and chief replies',
    'automaticApiRunnerParallelEnabled': True, 'verificationWithActiveApiEnabled': True,
    'sameProjectCliApiParallelEnabled': False, 'chiefAcceptanceWithActiveApiEnabled': True,
    'executionWorkerLimitGlobal': 2, 'backgroundOperationLimitGlobal': 4, 'verificationLimitPerProject': 1,
    'newJournal': False, 'apiWorkerAcceptanceAuthority': False,
    'verified': ['chief binds unchanged verified submission to original immutable history prefix',
        'single writer checks authority again before original Accept publication',
        'A accepted while B runs; C depending on A starts while B remains gated',
        'original ApplyIntent and Applied reused; lost acknowledgement reconciles original event without resubmission',
        'lost acknowledgement followed by reopen preserves one Accept and no new workflow events',
        'ApplyIntent without workflow event stays unresolved on active/idle/reopened owner; repeat entry never publishes Accept',
        'lost acknowledgement refreshes original workflow projection while management application remains unresolved',
        'stale response and forged snapshot/worker Accept rejected',
        'interleaved independent B completion preserves original A authority'],
    'mixedHostResults': results,
    'failuresPreserved': ['initial test attempted two simultaneous pending chief requests; original journal correctly rejected second; fixture sequenced stale response after first application/reopen, production single-pending protection unchanged'],
    'finalCheckScope': 'initial full default regression inherited for unchanged non-core paths; final core default and four chief scenarios rerun after applying the same no-resend guard to idle/reopened owners; final clippy and desktop build match final source',
    'unresolvedObservation': prior['unresolvedObservation'],
    'projectionSharingObservation': prior['projectionSharingObservation'],
    'remaining': ['Codex/OMP shared startup/ACK/cancel/recovery and full heterogeneous concurrent scheduling',
        'real heterogeneous coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'specializedCheckSeconds': {stage: float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', read(stage))[-1])
        for stage in ['runner', 'lifecycle4', 'core-final', 'verification', 'mixed']},
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': 453, 'rustIgnored': 163}, ensure_ascii=False))
