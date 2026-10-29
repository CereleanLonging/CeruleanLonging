"""Freeze 111: actual updater integration, not a new shipped release."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-update-interlock-20261012.json'
archive = reports / 'p6-update-interlock-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def readlog(p):
    b = p.read_bytes()
    return json.loads(b.decode('utf-16' if b.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig'))
prior_path = reports / 'p6-maintenance-interlock-20261012.json'; prior = load(prior_path)
for item in prior['records']:
    assert digest(root / item['snapshot']) == item['sha256']
package_path = root / prior['packageManifest']
assert digest(package_path) == prior['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
for item in prior['records']:
    if item['source'].startswith('crates/'):
        assert digest(root / item['source']) == item['sha256']
natives = [readlog(reports / f'p6-update-interlock-native{i}-20261012.log') for i in [1, 2]]
runs = [reports / Path(n['run']).name for n in natives]
for native, run in zip(natives, runs):
    assert native == load(run / 'result.json')
    assert native['status'] == 'passed' and native['cleanup'] == 'passed'
    assert all(native[k] for k in ['actualInstaller', 'actualUpdater', 'actualRollback', 'actualCommit', 'realReservationArchiveSharingFailure', 'terminalFinishDidNotReplayCommit', 'projectUnchanged'])
    assert all(p['exit'] == 0 for p in native['nativeProcesses'])
    assert digest(run / 'reviewed-maintenance-core.exe') == native['developmentHelperSha256']
assert natives[-1]['syntheticIncompletePreparationAbandoned'] and natives[-1]['missingReceiptExplicitGuardReadback']
legacy = readlog(reports / 'p6-update-interlock-legacy-20261012.log')
assert legacy['status'] == 'passed' and legacy['allTransactionBytesUnchanged']
cleanup = load(reports / 'p6-update-interlock-cleanup-20261012.json')
assert cleanup['remainingFixtureNonShellProcesses'] == 0
assert not cleanup['uninstallRegistrationPresent'] and not cleanup['productRegistrationPresent']
assert load(reports / 'p6-update-interlock-syntax-20261012.json')['status'] == 'passed'
stamp = datetime.now().astimezone().isoformat()
body = '''111 将110持久维护占用接入真实程序更新控制器。Prepare初始intent现在包含完整oldEntries及维护核心路径/摘要/能力版本；写入意图后、备份与暂存之前reserve。备份后及Apply/Commit、普通Rollback前重新核验原数据，之后原控制器继续持有数据/事务锁并执行既有安装登记、程序字节、备份检查。占用回执单独持久保存；回执缺失时可从原数据旁占用核对原意图身份，不以缺失回执断言动作未执行。

Rollback/Commit/Reconcile完成后先释放控制器持有的写锁与事务锁，再由仍被文件句柄固定且摘要已核验的维护核心独立release。释放失败不返回整体通过，保留原终态及占用。新增FinishMaintenance只接受终态，核对原计划、事件链、包、安装登记及实际程序/数据证据后收尾，不再提交或回滚。已归档的同一占用可只读核对；历史状态仍标currentDataVerified=false。AbandonPreparation要求明确原始intent摘要、无执行事件、原程序/数据/hold未变，归档占用并保留暂存现场；没有本地回执的已归档情况可明确提供GuardDigest核对。

新候选应声明maintenanceInterlockSchema=1并使用已审阅候选核心。为内部组合验证提供MaintenanceCore加SHA256，必须显式AllowUnsignedInternal、路径与摘要齐全、位于所有可变目录之外，并保持二进制及祖先路径只读固定。正式路径不接受该开发覆盖。旧事务缺少维护契约时可只读Review，新控制器拒绝隐式认领或改写旧意图；后续旧事务的变更处置需审阅原控制器与当时证据，不能把旧脚本当作支持新互斥的实现。

进行了两轮独立真实0.1.5安装到0.1.6更新组合验证，均使用哈希固定的110开发维护核心，第二轮覆盖最终脚本。实际Apply/回滚及版本登记恢复通过，原数据与hold保持。真实提交期间只读持有维护占用文件并拒绝删除共享，Commit实际完成并归档hold，但占用归档遇到真实Windows共享冲突；控制器返回失败，Review准确显示committed且reserved，开发核心启动被拒绝。释放测试句柄后显式FinishMaintenance成功，数据摘要保持，重复核对不重复提交/hold释放，项目读回一致。

第二轮另用完整原始意图与真实reserve建立明确的未完成准备夹具，不造prepared事件也不保存占用回执；实际AbandonPreparation成功，原数据不变，随后明确GuardDigest核查已归档结果通过。这是合成准备状态，不是杀死真实Prepare的证据。对既有旧事务只读审阅及拒绝新控制器Rollback通过，全部旧事务文件摘要保持。三份PowerShell脚本语法通过。Rust未改变，核对110源码摘要一致，继承其511通过/177默认忽略、严格clippy及构建，不无故重跑核心/基础接入。

两轮安装/卸载原进程均确认退出0，当前安装登记与夹具非shell进程均无残留，未关闭用户程序。第一轮在新增准备放弃用例及原数据复核之前执行，第二轮是最终脚本验证，不用第一轮代替最终证据。两轮记录均保留；没有模型调用或用户桌面动作。110冻结112项和0.1.6包摘要保持。

注意：冻结0.1.6本体不认识新维护占用；此次验证的启动/恢复阻塞由固定开发核心执行，不能宣称旧安装包自身已满足全部互斥。新版控制器源码尚未嵌入重建桌面程序，维护窗口也尚无FinishMaintenance/AbandonPreparation及恢复事务完整入口。版本字段暂仍0.1.6，源码/debug为未发行组合，不与旧桌面混用。下一步整合图形维护、统一升版能力和嵌入脚本，构建新候选并验新版安装运行链路；不得覆盖冻结同版本包。

完整P3/P4/P5/P6仍未通过，目标active。用户暂无独立Windows环境和签名证书，继续其他验收，不重复询问。干净机器/安装版完整React入口、统一程序数据事务、跨用户机器路径、其余真实中断/存储故障、签名/更新服务，以及首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察全部保持。WorkBuddy064不重发，不关机，无提交推送。

[111验收索引](../reports/p6-update-interlock-20261012.json)；[程序更新事务](WINDOWS-UPDATE-TRANSACTION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第019项（110）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第020项 / 111' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第020项（111），progress，真实更新控制器维护互斥/回滚/提交收尾通过，图形整合与新版发行待办，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方110为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as f:
    f.write('\n\n## 第020项 / 111 更新控制器维护互斥\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(WINDOWS-UPDATE-TRANSACTION-V1.md)', '(../../docs/WINDOWS-UPDATE-TRANSACTION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as f:
    f.write('\n\n## 111 实际更新控制器整合（未发行）\n\n' + stamp + '\n\n' + body)
note = '''\n\n111更新控制器已接110占用：初始完整intent、备份前reserve、关键动作前核验原数据、终态释放自身锁后独立release。新增`FinishMaintenance -PlanDigest`用于原终态占用收尾，新增`AbandonPreparation -IntentDigest`用于未开始准备；无回执的已归档占用可明确`-GuardDigest`核查。失败保留证据，不重发Commit/Rollback。旧事务只读可查，拒绝隐式接管。实际隔离安装/回滚/提交、真实占用归档共享失败及收尾通过；未完成准备采用明确合成夹具。\n\n当前仅脚本源码整合，0.1.5→0.1.6加固定开发维护核心验证不能代替新版候选；旧0.1.6自身不识别占用。正式候选须声明maintenanceInterlockSchema=1；独立helper覆盖仅显式内部验证接受。维护窗口尚需新收尾/准备放弃和恢复事务入口，之后统一升版、嵌入脚本和新包验证。见[111证据](../reports/p6-update-interlock-20261012.json)。\n'''
for name in ['WINDOWS-UPDATE-TRANSACTION-V1.md', 'MAINTENANCE-WINDOW-V1.md']:
    with (root / 'docs' / name).open('a', encoding='utf8') as f: f.write(note)
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 111：实际更新控制器互斥、回滚、提交占用释放失败后的独立收尾通过，源码尚未发行。\n\n- 下一步图形维护：程序FinishMaintenance/AbandonPreparation，恢复review/prepare/status/diagnose/apply/finish/abandon/inspect；保留明确审阅与未知状态核查。\n- 然后统一升版、能力声明、嵌入脚本和新候选，不覆盖冻结0.1.6；实际验证支持占用的新安装运行链路。\n- 旧0.1.6加开发helper的验证不等于旧包支持互斥，合成未完成准备不等于实际Prepare进程退出。\n- 本日卷002已到20项，后续新建带时间戳卷003。\n- 全部原P3/P4/P5/P6范围保持；暂无独立Windows及证书，继续其他验收。先读[最新交接](HANDOFF-LATEST.md)与[111证据](../reports/p6-update-interlock-20261012.json)。\n\n以下110及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 111）：实际更新控制器已接持久维护占用，隔离安装/回滚、真实提交收尾共享冲突及明确恢复通过。图形整合与统一升版待办，尚未发行；先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['scripts/manage-windows-update.ps1', 'scripts/check-update-interlock-20261012.ps1',
         'scripts/check-update-interlock-legacy-20261012.ps1', 'scripts/finalize-update-interlock-20261012.py',
         'scripts/test-windows-package.ps1', 'README.md', 'docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md', 'docs/WINDOWS-UPDATE-TRANSACTION-V1.md',
         'docs/MAINTENANCE-WINDOW-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-update-interlock-*-20261012.log'))
paths.update(reports.glob('p6-update-interlock-*-20261012.json'))
for run in runs: paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '111', 'at': stamp, 'status': 'actual-update-controller-interlock-and-terminal-release-recovery-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
          'nativeRuns': natives, 'finalNativeRun': natives[-1]['run'], 'legacy': legacy, 'cleanup': cleanup,
          'controllerIntegrated': True, 'maintenanceWindowIntegrated': False, 'newVersionedPackageAccepted': False,
          'shippedPackageChanged': False, 'rustUnchanged': True,
          'inheritedWorkspace': prior['workspace'], 'packageManifest': package_path.relative_to(root).as_posix(),
          'packageManifestSha256': digest(package_path), 'previousIndex': prior_path.relative_to(root).as_posix(),
          'previousIndexSha256': digest(prior_path), 'previousArchiveRecordsVerified': len(prior['records']),
          'remaining': ['maintenance window and unified new versioned package', 'actual incomplete Prepare interruptions and remaining storage faults', 'all original P3/P4/P5/P6 scope including clean machine/signing/providers/desktops/management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as f: json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
