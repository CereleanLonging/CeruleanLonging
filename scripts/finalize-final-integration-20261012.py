"""Freeze batch 075 once: explicitly approved final integration, local evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-final-integration-20261012.json'
archive = reports / 'p5-final-integration-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'
def checksum(path):
    return sha256(path.read_bytes()).hexdigest()
def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')
previous = reports / 'p5-dependency-hosts-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name, count in [('scope-final', 1), ('host-serial', 1), ('config-legacy', 3)]:
    assert f'{count} passed; 0 failed' in read(f'p5-integration-{name}-20261012.log')
native = read('p5-integration-host-serial-20261012.log')
for mode, dispatches in [('success', 1), ('bad-whole', 1), ('conflict', 0)]:
    assert f'integration scenario {mode}: passed, final dispatches {dispatches}, reopen events 0' in native
full = read('p5-integration-full-check-final-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert counts and 'test result: FAILED' not in full
assert 'pass 2' in full and 'pass 3' in full and 'built in' in full
for name in ['clippy-final', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-integration-{name}-20261012.log')
ui_runner = reports / 'p5-integration-ui-owned-20261012'
ui_value = json.loads((ui_runner / 'stdout.log').read_text(encoding='utf-8-sig').strip().splitlines()[-1])
assert ui_value['status'] == 'passed' and len(ui_value['cases']) == 12
ui = Path(ui_value['artifact'])
cleanup = json.loads((ui_runner / 'cleanup.json').read_text(encoding='utf-8-sig'))
assert cleanup['CleanupConfirmed'] and cleanup['RemainingProcesses'] == 0 and cleanup['ExitCode'] == 0
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/execution.rs', 'crates/domain/src/execution/workspaces.rs',
    'crates/domain/src/project_setup.rs', 'crates/domain/src/bin/export-contracts.rs',
    'crates/core/src/host.rs', 'crates/core/src/project_setup.rs',
    'crates/core/src/project_planning.rs', 'crates/core/src/project_runner.rs',
    'crates/core/src/worktree_configuration.rs', 'crates/core/src/integration_tests.rs',
    'crates/core/src/dependency_host_fixture.rs', 'crates/core/src/api_host_worktree_tests.rs',
    'crates/core/src/codex_worktree_tests.rs', 'crates/core/src/omp_host/tests/worktree.rs',
    'crates/core/src/git_workspaces/tests/approved.rs', 'crates/adapters/src/openai.rs',
    'apps/desktop/src/IntegrationFields.tsx', 'apps/desktop/src/WorktreeFields.tsx',
    'apps/desktop/src/ProjectPlanning.tsx', 'apps/desktop/src/WorkflowPlans.tsx',
    'apps/desktop/src/contracts.ts', 'tests/worktree-project-form.mjs',
    'scripts/finalize-final-integration-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-integration-*-20261012.log'))
paths.update(ui_runner / name for name in ['stdout.log', 'stderr.log', 'cleanup.json'])
paths.update(ui / name for name in ['result.json', 'entry.tsx', 'worktree-config.png', 'integration-config.png'])
assert all(p.is_file() for p in paths)
archive.mkdir()
records=[]
for path in sorted(paths):
    relative=path.relative_to(root)
    snapshot=archive/relative
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path,snapshot)
    value=checksum(path)
    assert checksum(snapshot)==value
    records.append({'source':relative.as_posix(),'snapshot':snapshot.relative_to(root).as_posix(),'sha256':value})
report={
    'batch':'075', 'at':datetime.now().astimezone().isoformat(), 'goalStatus':'active',
    'status':'approved-final-integration-local-checks-passed', 'fullP5Accepted':False, 'fullP6Accepted':False,
    'integrationTaskImplemented':True, 'integrationRouteVerified':'resident API with synthetic DeepSeek and manager replies',
    'scenarios':['independent branches to final delivery','partial checks pass, whole-project check fails','branch conflict blocks final dispatch'],
    'finalRestartNewEvents':0, 'scopeChecks':1, 'legacyConfigTests':3, 'uiScenarios':12,
    'rustDefaultPassed':sum(int(p) for p,_ in counts), 'rustDefaultIgnored':sum(int(i) for _,i in counts),
    'nodeTests':5, 'runtimeHashes':runtimes, 'realModelCalls':0, 'userDesktopActions':0,
    'parallelDispatchEnabled':False, 'heterogeneousWorkersInOneProjectVerified':False,
    'unresolvedObservation':'One Git rev-parse exited 0 but Windows Job remained active beyond the existing 100 ms drain and correctly stopped dispatch. Serial rerun passed; root cause not established. Preserve for process lifecycle/load acceptance.',
    'failuresPreserved':['test initially expected no startup record on merge conflict; corrected to retained reservation and zero dispatch','Git descendants-left-running observation','sandbox frontend spawn EPERM, elevated local build passed','initial test compilation errors','strict clippy large_enum_variant: boxed optional integration configuration; wire format preserved'],
    'remaining':['per-project multiple workers and parallel resource leases','heterogeneous integration and real efficiency','Git lifecycle reliability observation','full original P0-P6 requirements including real management, first-batch access, desktop reliability, install/update/rollback/migration/signing'],
    'previousIndex':previous.relative_to(root).as_posix(), 'previousIndexSha256':checksum(previous),
    'archiveScope':'selective source and local evidence, not a full reproducible repository', 'records':records,
}
with index.open('x',encoding='utf-8') as stream:
    json.dump(report,stream,ensure_ascii=False,indent=2);stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'rustPassed':report['rustDefaultPassed'],'rustIgnored':report['rustDefaultIgnored']},ensure_ascii=False))
