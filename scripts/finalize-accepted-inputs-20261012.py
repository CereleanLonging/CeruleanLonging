"""Freeze batch 072 once; accepted snapshot reader, not dependent dispatch."""
from pathlib import Path
from datetime import datetime
from hashlib import sha256
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-accepted-inputs-20261012.json'
archive = reports / 'p5-accepted-inputs-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'


def checksum(path):
    return sha256(path.read_bytes()).hexdigest()


def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')


previous = reports / 'p5-worktree-config-20261011.json'
for record in json.loads(previous.read_text(encoding='utf-8-sig'))['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
assert '6 passed; 0 failed' in read('p5-accepted-inputs-approved-final-20261012.log')
regression = read('p5-accepted-inputs-core-regression-20261012.log')
match = re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', regression)
assert match and int(match[1]) == 275
for name in ['clippy', 'core-build', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-accepted-inputs-{name}-20261012.log')
assert not read('p5-accepted-inputs-fmt-20261012.log').strip()
runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtimes = {name: checksum(root / name) for name in runtime_names}
assert len(set(runtimes.values())) == 1
names = [
    'crates/core/src/source_snapshot.rs', 'crates/core/src/accepted_inputs.rs',
    'crates/core/src/scheduler.rs', 'crates/core/src/git_workspaces/tests/approved.rs',
    'crates/core/src/git_workspaces/tests/approved/inputs.rs',
    'scripts/finalize-accepted-inputs-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-accepted-inputs-*.log'))
assert all(path.is_file() for path in paths)
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
    'batch': '072', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'accepted-dependency-snapshot-reader-checks-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'newNativeCases': 2, 'existingApprovedWorkspaceRegressionCases': 4,
    'coreDefaultPassed': int(match[1]), 'coreDefaultIgnored': int(match[2]),
    'realModelCalls': 0, 'userDesktopActions': 0,
    'acceptedSnapshotReaderImplemented': True,
    'dependentWorkspacePreparationEnabled': False,
    'dependencyOverlayPersisted': False,
    'parallelDispatchEnabled': False, 'integrationTaskImplemented': False,
    'scope': 'Real Git, filesystem locks, independent verifier and durable journal with synthetic worker/management decisions; mutable worker edits ignored only for accepted downstream snapshot reads',
    'inputLimits': {'aggregateFiles': 20000, 'aggregateBytes': 512 * 1024 * 1024},
    'unchangedEvidenceReused': ['071 UI/domain/frontend/IPC checks', 'prototype access and official model checks'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'runtimeHashes': runtimes, 'archiveScope': 'selective source/evidence snapshot', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'status': report['status']}, ensure_ascii=False))
