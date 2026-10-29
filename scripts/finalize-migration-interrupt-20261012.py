"""Freeze 103: real migration process termination and lost-response recovery."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root/'reports'
index = reports/'p6-migration-interrupt-20261012.json'
archive = reports/'p6-migration-interrupt-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(name):
    raw = (reports/name).read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous_path = reports/'p6-data-format-20261012.json'; previous = load(previous_path)
for entry in previous['records']:
    assert digest(root/entry['snapshot']) == entry['sha256'], entry['snapshot']
    if entry['source'].endswith('.rs') and entry['source'] != 'crates/core/src/data_format.rs':
        assert digest(root/entry['source']) == entry['sha256']
original = next(x for x in previous['records'] if x['source'] == 'crates/core/src/data_format.rs')
before = (root/original['snapshot']).read_text(encoding='utf8')
after = (root/original['source']).read_text(encoding='utf8')
for phase in ['before-create', 'pending-created', 'pending-synced', 'marker-published']:
    after = after.replace(f'        #[cfg(test)]\n        tests::interrupt_checkpoint("{phase}");\n', '')
start = after.index('    // Only the ignored, explicitly launched test child')
end = after.index('    fn fixture()', start)
after = after[:start] + after[end:]
assert after == before, 'Only test-only checkpoints and test helper may differ'
package_path = root/previous['packageManifest']; package = load(package_path)
assert digest(package_path) == previous['packageManifestSha256']
for entry in package['artifacts']:
    assert digest(package_path.parent/entry['path']) == entry['sha256']
native = json.loads(log('p6-migration-interrupt-native1-20261012.log'))
lost = json.loads(log('p6-migration-lost-ack-native1-20261012.log'))
for item in [native, lost]:
    assert item['status'] == 'passed' and item['cleanup'] == 'passed'
    assert load(reports/Path(item['run']).name/'result.json') == item
    assert not item['powerLossTested'] and not item['diskFullTested']
assert native['passed'] == 4 and native['instrumentedLibraryTestBuild'] and native['frozenRuntimeRecovery']
assert digest(Path(native['testExecutable'])) == native['testExecutableSha256']
assert [x['phase'] for x in native['cases']] == ['before-create', 'pending-created', 'pending-synced', 'marker-published']
for case in native['cases']:
    assert case['hasExited'] and case['exitCode'] != 0 and case['testOnlyCheckpoint']
    assert case['originalFilesUnchanged'] and case['originalLocationRestoreExact'] and case['legacyApprovedPlansRecovered']
assert lost['publicationObservedBeforeTermination'] and lost['hasExited'] and lost['exitCode'] != 0
assert not lost['instrumentedMigrationBuild'] and lost['migrationAckBytes'] == 0
assert lost['shippedRuntimeSha256'] == package['coreSha256']
assert lost['repeatMigrationRejectedWithoutMutation'] and lost['approvedPlansPreserved']
assert '3 passed; 0 failed; 1 ignored' in log('p6-migration-interrupt-unit-20261012.log')
assert 'Finished' in log('p6-migration-interrupt-clippy-20261012.log')
assert not log('p6-migration-interrupt-fmt-20261012.log').strip()
stamp = datetime.now().astimezone().isoformat()
body = '''103 补充五个真实进程终止场景，继续使用0.1.5，无生产逻辑/依赖/版本变更。四个场景在仅测试构建的迁移函数中设置明确停点：临时文件创建前、创建空文件后、写入同步后、正式改名发布后。子进程停点回执绑定PID、阶段和独立令牌；父进程实测原写锁仍被持有、解除hold拒绝，再通过原Popen进程句柄终止并等待退出。四个退出均已确认，码1。测试停点被cfg(test)排除，生产包内无这套环境变量或等待逻辑；不能把这些说成四个冻结发行程序的随机崩溃。

四个现场均保持原业务文件摘要与hold，未发布阶段的空/已同步临时文件被保留，状态读取、解除锁定和重复迁移拒绝；已发布阶段可以读回有效格式2。恢复拒绝覆盖现存目录；核对所有归属路径后保留完整中断目录，再由冻结0.1.5核心从备份恢复到原位置，全部文件摘要一致，原hold保持，显式解除后冻结0.1.4读取原批准计划通过。四项清理通过，未删除故障现场。

第五项使用冻结0.1.5安装包里的未插桩核心：预填满输出管道阻止迁移ACK，观察该进程仍存活时标记已发布并由另一只读调用核对格式与备份摘要；随后终止原进程并确认退出，实际ACK字节为0。重开读取结果一致，重复迁移拒绝且文件不变，原hold一直保留直到明确解除，原批准计划可读。只终止该脚本创建的进程，不操作用户应用，不把未收到返回值当作未执行而重发。

三个格式单元检查通过，专用子进程入口默认忽略（本次实际由驱动调用四次）；格式检查与严格workspace全targets clippy通过。源码比对确认相对102仅增加测试停点/测试入口，产品迁移算法没有变动，因此继承102完整workspace489通过/174忽略及构建/候选/维护窗口证据，不重复全套接入或打包。102冻结归档189项与0.1.5包全部摘要保持；没有生成同版本不同生产包。debug桌面/runtime仍是旧开发构建，不用于本批验收。

这五项是进程终止证据，尚不证明真实断电、磁盘满或所有存储故障。下一步继续迁移维护入口整合、磁盘满/剩余故障窗口及安装版完整入口，保留两类提交恢复按钮、签名/更新服务/干净机器和所有原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察。用户暂无证书，继续其他验收，不再询问。完整P3/P4/P5/P6仍未通过，目标active。真实模型/用户桌面动作0，WorkBuddy064不重发、不关机、不关闭用户程序、无提交推送。

[数据迁移说明](DATA-FORMAT-MIGRATION-V1.md)；[103验收索引](../reports/p6-migration-interrupt-20261012.json)。
'''
latest = root/'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第011项（102）' in old.splitlines()[2]
volume = root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第012项 / 103' not in volume.read_text(encoding='utf8')
readme = root/'README.md'; text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
updated = text[:start]+'最新进展（2026-10-12 / 103）：数据迁移四个明确阶段的真实测试进程终止/恢复，以及冻结0.1.5程序发布后ACK丢失处理通过；原事件、锁定和批准保持。生产包与102相同。断电/磁盘满、完整维护入口及正式发布等仍待验收。先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。'+text[end:]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第012项（103），progress，迁移进程终止与发布后响应丢失恢复通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方102为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第012项 / 103 迁移进程终止与响应丢失\n\n'+stamp+'\n\n'+body.replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)').replace('(../reports/', '(../../reports/'))
with (root/'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 103 迁移进程终止与响应丢失（限定范围通过）\n\n'+stamp+'\n\n'+body)
gates = root/'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 103：四个明确阶段的测试迁移进程终止恢复、冻结0.1.5发布程序迁移后响应丢失处理通过；原包摘要保持，完整阶段未通过。\n\n- 接下来推进迁移维护入口整合、磁盘满/剩余故障恢复；安装版完整入口、两类提交恢复控件仍待补。\n- 不把进程终止等同断电或磁盘满，不重复102已完成的正常迁移/包审查/维护窗口链。\n- 签名证书暂无，继续其他验收；全部原首批接入、真实管理/编程/效率等范围保持。\n- 先读[最新交接](HANDOFF-LATEST.md)与[103证据](../reports/p6-migration-interrupt-20261012.json)，WorkBuddy064不重发。\n\n以下102及更早为历史。\n\n'+gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme.write_text(updated, encoding='utf8')
names = ['crates/core/src/data_format.rs', 'crates/core/Cargo.toml', 'Cargo.lock',
         'scripts/check-migration-interrupt-20261012.py', 'scripts/check-migration-lost-ack-20261012.py',
         'scripts/finalize-migration-interrupt-20261012.py', 'README.md', 'docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'docs/DATA-FORMAT-MIGRATION-V1.md']
paths = {root/name for name in names}|{volume, package_path}
paths.update(reports.glob('p6-migration-interrupt-*-20261012.*'))
paths.update(reports.glob('p6-migration-lost-ack-*-20261012.log'))
for item in [native, lost]:
    paths.update(p for p in (reports/Path(item['run']).name).rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    rel = path.relative_to(root); target = archive/rel
    target.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, target)
    value = digest(path); assert digest(target) == value
    records.append({'source': rel.as_posix(), 'snapshot': target.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '103', 'at': stamp, 'status': 'migration-process-termination-and-lost-response-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
          'instrumentedCases': native, 'shippedLostResponseCase': lost, 'productionLogicChanged': False,
          'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path),
          'packageUnchanged': True, 'realModelCalls': 0, 'userDesktopActions': 0, 'publisherCertificateAvailable': False,
          'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path),
          'previousArchiveRecordsVerified': len(previous['records']), 'powerLossTested': False, 'diskFullTested': False,
          'remaining': ['migration maintenance UI/integration', 'remaining storage faults, disk full and power loss',
                        'installed entry through whole maintenance flow', 'Reconcile/AbandonCommit real UI fault flows',
                        'signing/update service/clean target', 'all original P3/P4/P5 provider/management/desktop/coding/efficiency requirements',
                        '075/085 unresolved observations'], 'records': records,
          'archiveScope': 'selected source and synthetic evidence; test executable hash recorded, not archived'}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'instrumentedCases': 4, 'shippedLostResponseCases': 1, 'goalStatus': 'active'}, ensure_ascii=False))
