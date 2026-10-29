"""Freeze batch 084 after all changed-path checks; never rewrite earlier evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-api-parallel-20261012.json'
archive = reports / 'p5-api-parallel-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(stage):
    return (reports / f'p5-api-parallel-{stage}-20261012.log').read_text(encoding='utf-8-sig')

previous = reports / 'p5-api-registry-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
assert len(prior['records']) == 55
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
ui_inherited = []
for record in ui['records']:
    if record['source'] in prior['inheritedUiSources']:
        assert checksum(root / record['source']) == record['sha256'], record['source']
        ui_inherited.append(record['source'])
assert len(ui_inherited) == 6
# The single writer's dispatch transaction itself is unchanged; budget reads and
# admission changed and are checked here rather than falsely inherited bytewise.
for name in ['crates/core/src/workflow_access.rs', 'crates/core/src/workflow_coordinator.rs',
             'crates/core/src/scheduler.rs', 'crates/domain/src/execution/workspaces.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
public = read('public-final')
assert '2 passed; 0 failed' in public
assert public.count('stale progress ignored, reopen events 0') == 2
assert 'public prefix gap: original management retained' in public
assert '6 passed; 0 failed' in read('budget')
assert 'actual hard-link window retains 4 approved requests' in read('budget')
full = read('full-check3')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 450, counts
assert sum(int(i) for _, i in counts) == 156, counts
assert '295 passed; 0 failed; 137 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '8 passed; 0 failed' in read('worktree')
assert '2 passed; 0 failed' in read('mixed')
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(stage)
cases = reports / 'p5-api-parallel-cases3-20261012'
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
names = {r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
names.update([
    'AGENTS.md',
    'crates/core/src/api_assignment_scope.rs', 'crates/core/src/api_startup.rs',
    'crates/core/src/omp_host.rs', 'crates/core/src/desktop_background.rs',
    'scripts/check.ps1', 'scripts/check-api-parallel-20261012.ps1',
    'scripts/finalize-api-parallel-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-api-parallel-*-20261012.log'))
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
    'batch': '084', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'public-api-parallel-admission-with-original-management-prefix',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'publicApiTests': 2, 'publicApiScenarios': 3, 'budgetTests': 6, 'worktreeTests': 8,
    'defaultCorePassed': 295, 'defaultCoreIgnored': 137,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes, 'inheritedUiSources': ui_inherited,
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'actual Git and public API worker starts with one writer; original management context, delayed Prepare and restart evidence; controlled provider/manager responses',
    'publicApiParallelEnabled': True, 'automaticProjectRunnerParallelEnabled': False,
    'executionWorkerLimitGlobal': 2, 'publicActiveApiLimitPerProject': 2,
    'legacyStartupVersion': 1, 'isolatedApiStartupVersion': 2, 'newJournal': False,
    'verified': ['public two-worker lifecycle including isolated cancel and stale progress',
        'original management history and configuration authority despite other-task completion',
        'delayed Prepare and non-adjacent Assign restart validation',
        'third API start rejected without startup reservation',
        'changed context, forged prefix, advanced target and nonisolated version2 rejected',
        'actual completion hard-link publication retains full reservation until pending cleanup',
        'prior budget races, ACK loss, identity guard, old worktree and serial mixed-host integration'],
    'mixedHostResults': results,
    'validationDriverIncident': 'initial Windows PowerShell wrapper stopped on child stderr warning; second run reached resident_client and failed with sandbox access denied as previously documented; logs preserved; final fresh full-check3/cases3 uses host permissions and checks actual child exit code',
    'rollbackCompatibility': 'new runtime accepts v1 and v2; old runtime does not accept v2, rollback must be validated under P6',
    'unresolvedObservation': prior['unresolvedObservation'],
    'budgetRule': 'full approved max_rounds until complete seal including its pending publication; unknown or local retirement never frees uncertain capacity',
    'remaining': ['automatic project runner parallel assignment and verification ownership',
        'Codex/OMP shared ownership, heterogeneous concurrent lifecycle and budgets',
        'real heterogeneous coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
