"""Freeze batch 078 once: cross-project explicit resources from durable authorities."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-resource-leases-20261012.json'
archive = reports / 'p5-resource-leases-sources-20261012'
assert not index.exists() and not archive.exists(), 'Never overwrite frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(name):
    return (reports / name).read_text(encoding='utf-8-sig')

previous = reports / 'p5-mixed-hosts-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8-sig'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
assert '4 passed; 0 failed' in read('p5-resource-leases-prepared-final-20261012.log')
full = read('p5-resource-leases-full-check-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 437
assert sum(int(i) for _, i in counts) == 148
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
for stage in ['clippy', 'desktop-build']:
    assert 'Finished `dev`' in read(f'p5-resource-leases-{stage}-20261012.log')
core_final = read('p5-resource-leases-core-final-20261012.log')
assert '283 passed; 0 failed; 129 ignored' in core_final
assert 'published_assignment_with_stale_projection_cannot_release_resources ... ok' in core_final
for stage in ['clippy-final', 'core-build-final', 'desktop-build-final']:
    assert 'Finished `dev`' in read(f'p5-resource-leases-{stage}-20261012.log')
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
names = [
    'crates/domain/src/workflow.rs', 'crates/core/src/host.rs',
    'crates/core/src/resource_leases.rs', 'crates/core/src/codex_management_bridge.rs',
    'crates/core/src/omp_host.rs', 'crates/core/src/api_host.rs',
    'crates/core/src/desktop_management.rs', 'crates/core/src/management_application.rs',
    'crates/core/src/scheduler.rs', 'crates/core/src/startup_journal.rs',
    'crates/core/src/api_startup.rs', 'crates/core/src/management_execution.rs',
    'scripts/check.ps1', 'scripts/finalize-resource-leases-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
paths = {root / name for name in names}
paths.update(reports.glob('p5-resource-leases-*-20261012.log'))
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
    'batch': '078', 'at': datetime.now().astimezone().isoformat(), 'goalStatus': 'active',
    'status': 'cross-project-explicit-resource-gates-and-replay-passed',
    'fullP5Accepted': False, 'fullP6Accepted': False,
    'authorities': ['existing task state including active-worker snapshot', 'Codex/OMP startup reservation', 'API startup reservation', 'uncommitted management assignment application'],
    'integrationPoints': ['Codex startup', 'OMP startup', 'API startup', 'desktop assignment', 'generic management assignment', 'background managed Prepare'],
    'targetedTests': 5, 'defaultCorePassed': 283, 'defaultCoreIgnored': 129,
    'rustDefaultPassed': sum(int(p) for p, _ in counts), 'rustDefaultIgnored': sum(int(i) for _, i in counts),
    'nodeTests': 5, 'runtimeHashes': runtimes,
    'finalCheckScope': 'full 437 Rust and 5 Node passed, then poisoned-ledger guard added with fifth resource test; final core 283 and strict workspace clippy/core/desktop rebuilt; unaffected domain/adapters/frontend/IPC checks inherited from full run',
    'realModelCalls': 0, 'userDesktopActions': 0,
    'executionScope': 'real durable ledgers, startup/application records and reopen; synthetic manager/active-worker state; no OS port or database locking',
    'parallelDispatchEnabled': False, 'resourceConfigurationUiImplemented': False,
    'generatedPlansDeclareResources': False, 'newLeaseJournal': False,
    'failuresPreserved': ['fixture resource identifiers used unsupported colon', 'fixture DispatchIntent omitted thread binding', 'test tried to call a private helper module'],
    'unresolvedObservation': prior['unresolvedObservation'],
    'remaining': ['approved profile/project resource declarations and UI', 'directory/channel resource strategy', 'per-attempt activity and single-writer coordination', 'parallel dispatch and unknown resource ownership', 'real heterogeneous coding and efficiency including failures', 'all original first-batch access, real OpenAI management, desktop reliability and P6 install/update/rollback/migration/signing'],
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': report['rustDefaultPassed'], 'rustIgnored': report['rustDefaultIgnored']}, ensure_ascii=False))
