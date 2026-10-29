"""Freeze batch 066 once; the archive is selective, not a repository backup."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-binding-20261011.json'
archive = reports / 'p5-worktree-binding-sources-20261011'
assert not index.exists() and not archive.exists(), 'Do not rewrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


previous = reports / 'p5-worktree-foundation-20261011.json'
for prior in [previous, reports / 'p4-workbuddy-coding-preparation-20261011.json']:
    for record in read(prior)['records']:
        assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']

checks_path = reports / 'p5-worktree-binding-check-results-20261011.json'
checks = read(checks_path)
assert all(checks[name] == 0 for name in ['nativeExitCode', 'fullCheckExitCode', 'clippyExitCode', 'desktopBuildExitCode'])
native = (reports / 'p5-worktree-binding-native-complete-20261011.log').read_text(encoding='utf-8-sig')
assert '4 passed; 0 failed; 0 ignored' in native
full = (reports / 'p5-worktree-binding-full-check-20261011.log').read_text(encoding='utf-8-sig')
results = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', full)
assert results and all(failed == '0' for _, failed, _ in results)
for name in ['clippy', 'desktop-build']:
    content = (reports / f'p5-worktree-binding-{name}-20261011.log').read_text(encoding='utf-8-sig')
    assert 'Finished `dev`' in content

runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {p: checksum(root / p) for p in runtime_names}
assert len(set(runtimes.values())) == 1

sources = [
    'crates/core/src/lib.rs', 'crates/core/src/git_workspaces.rs',
    'crates/core/src/git_workspaces/tests.rs', 'crates/core/src/git_workspaces/tests/approved.rs',
    'crates/core/src/git_workspaces/validation_fixture.rs', 'crates/core/src/scheduler.rs',
    'crates/core/src/artifacts.rs', 'crates/core/src/source_snapshot.rs', 'crates/core/src/verifier.rs',
    'crates/domain/src/execution.rs', 'crates/domain/src/execution/workspaces.rs',
    'crates/domain/src/bin/export-contracts.rs', 'apps/desktop/src/contracts.ts',
    'apps/desktop/src/WorkflowPlans.tsx', 'scripts/check.ps1',
    'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/HANDOFF-LATEST.md',
    'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'scripts/finalize-worktree-binding-20261011.py',
]
files = {root / p for p in sources}
# Include all existing struct constructors amended with the optional fields.
for path in (root / 'crates').rglob('*.rs'):
    content = path.read_text(encoding='utf-8-sig')
    if 'workspace_proof: None' in content or 'worktree: None' in content:
        files.add(path)
files.update(reports.glob('p5-worktree-binding-*-20261011.log'))
files.add(reports / 'p5-worktree-contract-export-20261011.log')
files.add(checks_path)
assert all(path.is_file() for path in files)

archive.mkdir()
records = []
for path in sorted(files):
    relative = path.relative_to(root)
    snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, snapshot)
    value = checksum(path)
    assert checksum(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})

report = {
    'batch': '066', 'at': datetime.now().astimezone().isoformat(),
    'status': 'approved-worktree-core-workflow-checks-passed', 'goalStatus': 'active',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'approvedWorktreeNativeCases': 4, 'inheritedFoundationCases': 7,
    'defaultRustPassed': sum(int(passed) for passed, _, _ in results),
    'defaultRustIgnored': sum(int(ignored) for _, _, ignored in results),
    'checks': checks, 'runtimeHashes': runtimes,
    'realModelCalls': 0, 'desktopActions': 0, 'receipts': 'synthetic worker and management evidence',
    'realFilesystemAndIndependentVerifier': True,
    'approvalAndArtifactBindingIntegrated': True, 'hostDispatchIntegrated': False,
    'configurationUiEnabled': False, 'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'recoveryScope': 'simulated missing-readiness and workflow reopen; not process crash or power loss',
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
