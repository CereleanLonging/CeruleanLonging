"""Freeze 106: Windows I/O lock failures, explicitly not disk exhaustion."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging')
reports = root / 'reports'
index = reports / 'p6-migration-io-20261012.json'
archive = reports / 'p6-migration-io-sources-20261012'
assert not index.exists() and not archive.exists(), 'Do not overwrite frozen evidence'

def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(path):
    raw = path.read_bytes()
    if raw.startswith((b'\xff\xfe', b'\xfe\xff')):
        return raw.decode('utf-16')
    try:
        return raw.decode('utf-8-sig')
    except UnicodeDecodeError:
        return raw.decode('gb18030')

previous_path = reports / 'p6-commit-window-20261012.json'
previous = load(previous_path)
for entry in previous['records']:
    assert digest(root / entry['snapshot']) == entry['sha256'], entry['snapshot']
baseline = load(reports / 'p6-data-window-20261012.json')
for entry in baseline['records']:
    assert digest(root / entry['snapshot']) == entry['sha256']
    name = entry['source']
    if name.startswith(('crates/', 'apps/')) or name in ['Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json', 'scripts/maintenance-window.ps1', 'scripts/manage-windows-update.ps1', 'scripts/test-windows-package.ps1']:
        if name == 'crates/core/src/data_format.rs':
            before = (root / entry['snapshot']).read_text(encoding='utf8')
            after = (root / name).read_text(encoding='utf8')
            assert before.split('#[cfg(all(test, windows))]')[0] == after.split('#[cfg(all(test, windows))]')[0]
            assert before[before.index('    fn fixture()'):] == after[after.index('    fn fixture()'):]
        else:
            assert digest(root / name) == entry['sha256'], name
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for entry in load(package_path)['artifacts']:
    assert digest(package_path.parent / entry['path']) == entry['sha256']
native = load(reports / 'migration-io-failure-qx4wju7x/result.json')
assert native['status'] == 'passed' and native['cleanup'] == 'passed' and native['passed'] == 2
assert native == json.loads(log(reports / 'p6-migration-io-native1-20261012.log'))
assert native['realWindowsIoFailure'] and native['instrumentedLibraryTestBuild']
assert not native['realChildTermination'] and not native['diskFullTested'] and not native['powerLossTested']
assert digest(Path(native['testExecutable'])) == native['testExecutableSha256']
run = reports / Path(native['run']).name
for case in native['cases']:
    assert case['hasExited'] and case['testExitCode'] == 0
    assert case['originalFilesUnchanged'] and case['originalLocationRestoreExact'] and case['legacyApprovedPlansRecovered']
    assert case['windowsError'] == (33 if case['phase'] == 'pending-created' else 32)
    assert (run / case['phase'] / 'io-error.txt').read_text(encoding='utf8') == case['migrationReturnedError']
    assert not case['automaticPreservationImplemented']
tests = log(reports / 'p6-migration-io-tests-20261012.log')
assert '3 passed; 0 failed; 2 ignored' in tests
assert 'Finished `dev` profile' in log(reports / 'p6-migration-io-clippy-20261012.log')
stamp = datetime.now().astimezone().isoformat()
body = '''106 补充迁移真实文件I/O失败后的保护与恢复。生产迁移算法、维护工具、依赖及冻结0.1.6包未改变；仅测试构建增加带令牌的停点恢复与预期错误子进程入口，复用103已验证的停点和104的冻结恢复核心。

第一例在临时文件创建后，由父进程持有该文件首1MiB的Windows字节范围锁，再恢复原迁移子进程。真实write_all返回os error 33，空临时文件留下。第二例在写入同步后，以不共享删除权限的真实文件句柄占用临时文件，再恢复原迁移；实际禁止覆盖改名返回os error 32，已写入的临时文件留下。未替换写入函数、伪造错误返回或终止迁移进程；测试子进程在确认迁移返回错误后正常退出0，不表示迁移成功。两个测试PID分别7512和14248。

两例原业务文件与hold摘要保持、正式标记未发布。冻结0.1.6格式读取、解锁、重复迁移及快照加载均拒绝，现存目标恢复拒绝且文件未变。驱动确认目录属于本次夹具后保留整个故障现场，再由冻结0.1.6恢复到原位置，所有文件摘要与备份一致；明确解除恢复出的hold后，冻结0.1.4读回原批准计划完全一致。两个测试进程已退出，测试文件锁已释放，清理通过，没有模型或用户桌面动作。

迁移专项3通过、2个专用入口默认忽略；本次驱动明确运行新增入口2次。格式检查及严格workspace全targets clippy通过。与104源码比对确认生产部分相同，105归档674项、104归档及冻结0.1.6包摘要保持；不重复正常接入、完整回归或打包。新增测试不是发行程序在随机时刻发生故障的证据。

本次只补Windows锁冲突下的真实写入/发布失败，不等同真实磁盘满、同步落盘失败、物理断电或全部存储故障通过。完整故障目录仍由验证驱动保留，产品尚未提供自动保留恢复。用户暂无独立Windows测试环境及代码签名证书，按明确指示继续其他验收，不重复询问、不搬动当前账户已有工作台数据。

完整P3/P4/P5/P6仍未通过，目标active。下一步推进可审阅的自动保留恢复事务并继续存储故障矩阵；安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务保留未验收。原全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察范围保持。WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[106验收索引](../reports/p6-migration-io-20261012.json)；[数据迁移说明](DATA-FORMAT-MIGRATION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf8')
assert '卷002第014项（105）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第015项 / 106' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第015项（106），progress，真实迁移写入/发布锁冲突与备份恢复通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方105为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第015项 / 106 迁移写入与发布故障\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 106 迁移I/O故障（限定范围通过）\n\n' + stamp + '\n\n' + body)
with (root / 'docs/DATA-FORMAT-MIGRATION-V1.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n106新增两个真实Windows文件锁冲突场景：创建临时文件后字节范围锁导致写入返回33；同步后删除共享冲突导致发布返回32。测试构建只提供停点与恢复，写入/改名仍走原OS调用。两例保持hold与原业务字节，冻结0.1.6拒绝继续并精确备份恢复，0.1.4读回原批准计划通过。完整目录保留由测试驱动完成；不是磁盘满、断电或自动保留恢复验收。详见[106证据](../reports/p6-migration-io-20261012.json)。\n')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 106：真实迁移写入/发布锁冲突及恢复通过，冻结0.1.6产品保持。\n\n- 下一步补自动保留恢复事务和剩余存储故障；本轮不是磁盘满或断电验收。\n- 安装版完整入口/干净机器、签名仍待环境；用户已说明暂无独立环境及证书，继续其他验收，不重复询问。\n- 全部原始首批接入/真实管理/两桌面/编程效率范围保持，完整P3/P4/P5/P6未通过。\n- 先读[最新交接](HANDOFF-LATEST.md)与[106证据](../reports/p6-migration-io-20261012.json)。\n\n以下105及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'
text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 106）：迁移真实Windows写入/发布锁冲突及原位置精确恢复通过，冻结0.1.6包保持。磁盘满/断电、自动保留恢复及原全部阶段门槛继续验收；先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['scripts/check-migration-io-20261012.py', 'scripts/finalize-migration-io-20261012.py', 'crates/core/src/data_format.rs', 'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', 'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-migration-io-*-20261012.*'))
paths.update(path for path in run.rglob('*') if path.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '106', 'at': stamp, 'status': 'windows-migration-write-and-publish-lock-failures-recovered', 'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False, 'result': native, 'productionAlgorithmUnchanged': True, 'packageUnchanged': True, 'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path), 'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path), 'previousArchiveRecordsVerified': len(previous['records']), 'diskFullAccepted': False, 'powerLossAccepted': False, 'automaticPreservationAccepted': False, 'publisherCertificateAvailable': False, 'isolatedWindowsEnvironmentAvailable': False, 'tests': {'migrationUnitPassed': 3, 'dedicatedChildEntrypointsIgnoredByDefault': 2, 'newChildActualRuns': 2, 'format': 'passed', 'strictWorkspaceClippy': 'passed'}, 'remaining': ['automatic preservation/restore transaction', 'real disk-full/sync failure/power-loss matrix', 'unified program-data transaction', 'installed React full flow and clean machine', 'cross-user/path/machine', 'signing/update service', 'all original provider/CLI/agent/desktop/management/coding/efficiency/budget gates; 075/085 observations'], 'records': records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
