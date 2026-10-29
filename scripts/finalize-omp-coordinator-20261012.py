"""Freeze batch 088 only after final local checks; preserve prior evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-omp-coordinator-20261012.json'
archive = reports / 'p5-omp-coordinator-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(stage):
    raw = (reports / f'p5-omp-coordinator-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous = reports / 'p5-shared-acceptance-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
assert len(prior['records']) == 99
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name in ['crates/core/src/api_execution.rs', 'crates/core/src/api_startup.rs',
             'crates/core/src/omp_host.rs', 'crates/core/src/managed_assignment.rs',
             'crates/core/src/codex_management_bridge.rs',
             'crates/core/src/git_workspaces.rs', 'crates/domain/src/execution/workspaces.rs',
             'crates/core/src/verifier.rs', 'crates/core/src/verifier/process.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
startup_prior = json.loads((reports / 'p5-resource-leases-20261012.json').read_text(encoding='utf-8'))
startup_record = next(r for r in startup_prior['records'] if r['source'] == 'crates/core/src/startup_journal.rs')
assert checksum(root / startup_record['source']) == checksum(root / startup_record['snapshot']) == startup_record['sha256']
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

full = read('full')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 456, counts
assert sum(int(i) for _, i in counts) == 165, counts
assert '300 passed; 0 failed; 146 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
for stage, count in [('protocol2', 3), ('budget', 8), ('worktree', 4), ('dag', 1), ('mixed', 2)]:
    assert f'{count} passed; 0 failed' in read(stage), stage
for stage in ['clippy', 'desktop']:
    assert 'Finished `dev`' in read(stage), stage
cases = reports / 'p5-omp-coordinator-cases-20261012'
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
    'AGENTS.md', 'crates/core/src/async_omp.rs', 'crates/core/src/omp_execution.rs',
    'crates/core/src/omp_execution/tests.rs', 'crates/core/src/omp_execution/tests/coordinated.rs',
    'crates/core/src/coordinated_omp.rs', 'crates/core/src/omp_workflow_access.rs',
    'crates/core/src/omp_host.rs', 'crates/core/src/omp_process.rs',
    'crates/core/src/startup_journal.rs',
    'scripts/check.ps1', 'scripts/check-omp-coordinator-20261012.ps1',
    'scripts/finalize-omp-coordinator-20261012.py', 'scripts/finalize-omp-coordinator-docs-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/NEXT-STAGE-GATES.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-omp-coordinator-*-20261012.log'))
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
    'batch': '088', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'omp-production-worker-uses-scoped-single-writer-client',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'defaultCorePassed': 300, 'defaultCoreIgnored': 146,
    'rustDefaultPassed': 456, 'rustDefaultIgnored': 165, 'nodeTests': 5,
    'protocolTests': 3, 'budgetTests': 8, 'ompWorktreeTests': 4, 'ompDagTests': 1, 'mixedHostTests': 2,
    'runtimeHashes': runtimes, 'inheritedUiSources': prior['inheritedUiSources'],
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'actual Git, controlled OMP/CLI child processes, file tools, original verifier and journals; synthetic provider and chief replies',
    'sameProjectCliApiParallelEnabled': False, 'ompHostRegistryStillProjectExclusive': True,
    'automaticApiRunnerParallelEnabled': True, 'verificationWithActiveApiEnabled': True,
    'chiefAcceptanceWithActiveApiEnabled': True, 'newJournal': False,
    'executionWorkerLimitGlobal': 2, 'backgroundOperationLimitGlobal': 4,
    'apiWorkerAcceptanceAuthority': False, 'ompWorkerAcceptanceAuthority': False,
    'verified': ['OMP production execution uses scoped client; original writer returns after client exits',
        'network wait and Git preparation do not own writer; original RPC, tools and terminal proofs retained',
        'OMP commands and paths bound to approved adapter, project, revision, task and attempt',
        'OMP cannot approve or verify/accept, API cannot acquire OMP paths or command capability',
        'one last request contested by stale API and OMP clients publishes exactly one dispatch',
        'API unsealed rounds and unknown CLI dispatch preserve original request capacity',
        'lost acknowledgement and interrupted event publication retain one original intent without resend',
        'original management double ACK, cancellation, worktree, dependency and reopen checks pass'],
    'mixedHostResults': results,
    'failuresPreserved': ['initial unused import warning removed',
        'initial protocol test callback temporary lifetime failed compilation; named observer fixes fixture, production not relaxed'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'projectionSharingObservation': prior['projectionSharingObservation'],
    'remaining': ['shared host registry and attempt-keyed OMP startup/ACK/cancel/recovery before public OMP/API concurrency',
        'Codex scoped execution migration and heterogeneous automatic scheduling',
        'real heterogeneous coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'specializedCheckSeconds': {stage: float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', read(stage))[-1])
        for stage in ['protocol2', 'budget', 'worktree', 'dag', 'mixed']},
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': 456, 'rustIgnored': 165}, ensure_ascii=False))
