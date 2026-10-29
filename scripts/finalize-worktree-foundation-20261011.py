"""Freeze batch 065 once; archives are selective, not a complete repository."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-worktree-foundation-20261011.json'
archive = reports / 'p5-worktree-sources-20261011'
assert not index.exists() and not archive.exists(), 'Do not rewrite frozen evidence'

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

def read(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))

native = reports / 'p5-worktree-native-final-20261011.log'
assert '7 passed; 0 failed; 0 ignored' in native.read_text(encoding='utf-8-sig')
assert (reports / 'p5-worktree-fmt-20261011.log').stat().st_size == 0
for name in ['clippy-final', 'core-build', 'desktop-build']:
    assert 'Finished `dev`' in (reports / f'p5-worktree-{name}-20261011.log').read_text(encoding='utf-8-sig')
previous = reports / 'p4-workbuddy-coding-preparation-20261011.json'
for item in read(previous)['records']:
    assert digest(root / item['snapshot']) == item['sha256'], item['snapshot']
git = read(reports / 'p5-worktree-git-20261011.json')
assert digest(Path(git['path'])) == git['sha256']
runtime_paths = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {p: digest(root / p) for p in runtime_paths}
assert len(set(runtimes.values())) == 1
sources = [
    'crates/core/src/lib.rs', 'crates/core/src/git_workspaces.rs', 'crates/core/src/git_workspaces/tests.rs',
    'crates/core/src/artifacts.rs', 'crates/core/src/verifier.rs', 'crates/core/src/verifier/process.rs',
    'crates/core/src/scheduler.rs', 'crates/domain/src/execution.rs',
    'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md',
    'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md',
    'scripts/finalize-worktree-foundation-20261011.py',
]
files = {root / p for p in sources}
files.update(reports.glob('p5-worktree-*-20261011.log'))
files.add(reports / 'p5-worktree-git-20261011.json')
archive.mkdir()
records = []
for source in sorted(files):
    relative = source.relative_to(root)
    snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, snapshot)
    checksum = digest(source)
    assert digest(snapshot) == checksum
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': checksum})
report = {
    'batch': '065', 'at': datetime.now().astimezone().isoformat(), 'status': 'worktree-foundation-tests-passed',
    'goalStatus': 'active', 'fullP5Accepted': False, 'fullP6Accepted': False,
    'realGitCases': 7, 'realModelCalls': 0, 'desktopActions': 0,
    'hostDispatchIntegrated': False, 'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'recoveryScope': 'simulated interrupted-publication windows on real Git repositories; not process crash/power-loss',
    'workspaceInput': 'exact pinned commit raw blobs; uncommitted changes and checkout filters not included',
    'preservedGates': 'scheduler still enforces project-root workspace until approval/attempt/execution/artifact bindings are integrated',
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous),
    'git': git, 'runtimeHashes': runtimes, 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
