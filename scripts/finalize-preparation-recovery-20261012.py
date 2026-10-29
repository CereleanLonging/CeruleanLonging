"""Freeze 109: interrupted preparation guidance and byte-preserving abandonment."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-preparation-recovery-20261012.json'
archive = reports / 'p6-preparation-recovery-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous_path = reports / 'p6-recovery-inspect-20261012.json'; previous = load(previous_path)
for item in previous['records']:
    assert digest(root / item['snapshot']) == item['sha256']
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
native = json.loads(log(reports / 'p6-preparation-recovery-native1-20261012.log'))
run = reports / Path(native['run']).name
assert load(run / 'result.json') == native
assert native['status'] == 'passed' and native['cleanup'] == 'passed' and not native['pending']
assert native['realChildTerminations'] == 3 and native['testOnlyCheckpoints']
assert len(native['cases']) == 4 and len(native['compatibility']) == 2
assert all(x['verifiedReadOnly'] and x['schema'] == 1 for x in native['compatibility'])
assert digest(root / 'target/debug/cerulean-core.exe') == native['developmentCoreSha256']
for case in native['cases']:
    assert case['originalFilesUnchanged'] and case['holdKept']
    if case['realChildTermination']:
        assert case['exitCode'] != 0 and case['testOnlyCheckpoint']
        receipt = load(run / case['phase'] / 'terminated.json')
        assert receipt['hasExited'] and receipt['pid'] == case['pid']
assert '14 passed; 0 failed; 1 ignored' in log(reports / 'p6-preparation-recovery-tests2-20261012.log')
assert '1 passed; 0 failed' in log(reports / 'p6-preparation-recovery-cli-20261012.log')
for name in ['clippy', 'build']:
    assert 'Finished `dev` profile' in log(reports / f'p6-preparation-recovery-{name}-20261012.log')
assert (reports / 'p6-preparation-recovery-fmt-20261012.log').exists()
stamp = datetime.now().astimezone().isoformat()
body = '''109 修复恢复准备的两个故障处置缺口。prepare先在唯一initializing目录写入完整计划和同步后的空操作锁，再禁止覆盖地发布同级保护目录；原数据写锁与维护hold保持。计划写入期间退出只留下未发布元数据，不先制造无法解析的启动保护。保护发布后再复制副本、核验并写prepared记录；副本或prepared失败时已有可核验计划可供明确放弃。未发布目录不自动删除，也不以文件时间推断进程已死。

新增只读recovery diagnose，分别报告保护是否存在、活动事务、准备标记有效/缺失/损坏、未发布元数据目录及下一步入口。原目录精确且尚无移动/恢复意图时才提供放弃准备入口；计划损坏、写锁繁忙、已有移动意图或收尾意图时保留阻塞并指向原证据审阅，不自动删除、解锁、启动或重试。旧开发现场被外部损坏的plan或缺失操作锁仍不能凭猜测自动解除，诊断明确显示人工证据审阅；本轮没有冒充完成任意损坏修复。

finalization写入schema2，事务记录按原始字节封存摘要，使空文件、截断JSON或错误prepared值也能原样保留后明确abandon。完成恢复仍逐条解析prepared/move/restore语义回执，不能用原始字节哈希代替成功条件。读取继续支持schema1，原收尾意图已存在时沿用其schema和目的地；不会重写旧归档。记录读取/写入上限及普通文件/链接检查保留。

恢复专项14通过、专用中断子进程入口默认忽略1；CLI参数1项、格式检查、严格workspace全targets clippy及开发核心构建通过。三个真实子进程退出分别在测试构建的计划文件刚创建、保护目录刚发布、副本校验后停点：PID8724、24532、25884均经原进程句柄终止并确认退出1。停点只存在测试构建，正常开发核心负责后续诊断和处置，不能说成冻结发行程序随机崩溃。

首例保留空plan的initializing目录，原hold及全部原文件保持、无保护目录，正常核心可以重新给出相同审阅但没有自动重做准备。后两例诊断准确显示prepared缺失、允许审阅放弃，apply拒绝；明确abandon后全部原记录/暂存文件摘要保持，只新增finalization回执，原hold不解除。第四例明确构造截断prepared JSON，用正常CLI诊断、拒绝apply、原样归档并核验通过。另对108的两份schema1真实归档只读检查，仍可核验且未改写任何字节。四例清理通过，没有模型或用户桌面动作。

108冻结归档118项及原0.1.6安装包摘要保持。仅开发核心/CLI更新，版本字段暂仍0.1.6，旧debug桌面/sidecar未同步；尚未打包发行，不能混用或把开发证据算成安装版完成。继承107/108未改的基础接入、核心启动与备份证据，不重复基础连接；真实磁盘满/同步失败/物理断电仍待验收。

完整P3/P4/P5/P6未通过，目标active。下一步优先程序更新控制器与数据恢复的双向互斥、图形维护整合、统一升版及候选包，再完成剩余真实事务中断/存储/安装验收。用户暂无独立Windows环境和签名证书，继续其他验收，不重复询问。原安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务，以及全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察保持。WorkBuddy064不重发，不关机、不关闭用户程序，无提交推送。

[109验收索引](../reports/p6-preparation-recovery-20261012.json)；[数据迁移说明](DATA-FORMAT-MIGRATION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第017项（108）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第018项 / 109' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第018项（109），progress，准备中断诊断/保留放弃与旧归档兼容通过，尚未发行，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方108为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第018项 / 109 准备中断与保留放弃\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 109 残缺准备处置（开发核心限定范围通过）\n\n' + stamp + '\n\n' + body)
with (root / 'docs/DATA-FORMAT-MIGRATION-V1.md').open('a', encoding='utf8') as stream:
    stream.write('''\n\n109新增 `recovery diagnose`，只读显示活动保护、prepared状态、未发布initializing目录和处置入口。准备现在先写完整计划与操作锁再发布保护，提前退出不留下不可解析的新保护目录。副本/准备标记损坏时，满足原目录未变且无移动意图的条件后，可明确abandon并保留原始字节；schema2封存原始记录，schema1旧归档继续可读。旧损坏plan/缺操作锁或已有移动意图不自动放弃，须审阅原证据。未发布元数据不自动清理，也不据此断言原进程已退出。三个测试构建停点的真实子进程终止、一个截断JSON夹具及两份旧归档兼容通过；尚未发行。详见[109证据](../reports/p6-preparation-recovery-20261012.json)。\n''')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 109：残缺准备诊断/保留放弃与schema1兼容通过，尚未发行。\n\n- 下一步优先补程序更新控制器与恢复事务双向互斥、图形维护、统一升版/候选包；再补其他真实中断/存储/安装门槛。\n- 旧损坏计划或缺操作锁仍需原证据审阅，不用未知状态自动解除；真实磁盘满/同步失败/断电仍待验证。\n- 冻结0.1.6包保持，旧debug桌面/sidecar不能混用；暂无独立Windows及签名证书，继续其他验收，不重复询问。\n- 全部原P3/P4/P5/P6范围保持，先读[最新交接](HANDOFF-LATEST.md)与[109证据](../reports/p6-preparation-recovery-20261012.json)。\n\n以下108及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8')
start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 109）：开发核心新增准备中断诊断，完整元数据发布后才建立保护；残缺prepared可原样保留并明确放弃。三次测试构建中断、实际CLI处置和旧归档兼容通过。尚未发行，先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['crates/core/src/backup_recovery.rs', 'crates/core/src/workbench_cli.rs', 'scripts/check-preparation-recovery-20261012.py', 'scripts/finalize-preparation-recovery-20261012.py', 'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', 'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-preparation-recovery-*-20261012.log'))
paths.update(reports.glob('p6-preparation-recovery-*-20261012.jsonl'))
paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '109', 'at': stamp, 'status': 'interrupted-preparation-diagnosis-preserving-abandonment-and-legacy-seal-verified', 'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False, 'native': native, 'recoveryTestsPassed': 14, 'ignoredDedicatedChildEntrypoints': 1, 'cliTestsPassed': 1, 'strictClippy': 'passed', 'developmentBuild': 'passed', 'shippedRecoveryAccepted': False, 'shippedPackageChanged': False, 'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path), 'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path), 'previousArchiveRecordsVerified': len(previous['records']), 'remaining': ['program updater/recovery interlock', 'maintenance window and versioned release', 'legacy corrupted plan or missing lock needs original evidence review', 'remaining real interruption/storage faults', 'all original P3/P4/P5/P6 gates including installed React/clean machine/signing/providers/desktops/real management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
