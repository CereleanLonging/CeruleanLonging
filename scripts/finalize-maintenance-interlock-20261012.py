"""Freeze batch 110; distinguish core interlock from unintegrated update controller."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging')
reports = root / 'reports'
index = reports / 'p6-maintenance-interlock-20261012.json'
archive = reports / 'p6-maintenance-interlock-sources-20261012'
assert not index.exists() and not archive.exists()

def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous_path = reports / 'p6-preparation-recovery-20261012.json'
previous = load(previous_path)
for item in previous['records']:
    assert digest(root / item['snapshot']) == item['sha256']
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
native = json.loads(log(reports / 'p6-maintenance-interlock-native1-20261012.log'))
run = reports / Path(native['run']).name
assert load(run / 'result.json') == native
assert native['status'] == 'passed' and native['cleanup'] == 'passed' and not native['pending']
assert native['powershellOwnerExit'] == 0
for field in ['reservationSurvivedControllerExit', 'liveExclusiveTransactionLockRejectedRelease',
              'bothAcquisitionOrdersRejectedOverlap', 'originalDataBytesUnchanged',
              'originalProgramBytesUnchanged', 'originalHoldPreserved',
              'releasedGuardReadbackVerified', 'terminalRollbackJournalIsSynthetic']:
    assert native[field]
assert not native['actualInstallerOrUpdateControllerTested']
assert digest(root / 'target/debug/cerulean-core.exe') == native['developmentCoreSha256']
process_check = load(reports / 'p6-maintenance-interlock-process-check-20261012.json')
assert process_check['remainingNonShellProcesses'] == 0
counts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',
                    log(reports / 'p6-maintenance-interlock-workspace-20261012.log'))
totals = [sum(int(row[i]) for row in counts) for i in range(3)]
assert totals == [511, 0, 177]
assert '6 passed; 0 failed' in log(reports / 'p6-maintenance-interlock-tests2-20261012.log')
for name in ['clippy', 'build']:
    assert 'Finished `dev` profile' in log(reports / f'p6-maintenance-interlock-{name}-20261012.log')
assert not log(reports / 'p6-maintenance-interlock-fmt-20261012.log').strip()
candidate = json.loads(log(reports / 'p6-maintenance-interlock-candidate-20261012.log'))
assert candidate['status'] == 'passed' and candidate['passed'] == 13
assert all(item['passed'] for item in candidate['checks'])
stamp = datetime.now().astimezone().isoformat()
body = '''110 新增开发核心的持久维护占用，补程序更新与数据恢复之间的互斥基础。maintenance reserve绑定原数据路径、当前SID、原hold、事务目录、原始intent字节摘要及原程序清单；在原数据写锁内拒绝已有恢复保护，检查完整原程序后发布同级占用记录。控制进程退出不会丢失该记录。恢复prepare/apply/finish、核心启动及普通解除路径拒绝绕过占用；反方向先有恢复保护时程序占用也拒绝。损坏或不可读的占用保留阻塞，不依据进程时间或文件时间自动解除。

新增maintenance status/check/release/abandon离线命令，要求明确摘要。未开始的准备只在原数据/原程序/原hold仍精确且没有更新事件、搬移目录时允许明确放弃。已执行的事务须独立取得transaction.lock、验证原intent及plan摘要、事件序号和哈希链、终态、实际程序字节及原备份。回滚保持原hold；提交要求原hold已移走、恰好一条新增匹配归档及其他原数据不变。释放只归档占用证据，不重做安装、修改登记、解除hold或启动服务。带摘要status可在返回丢失后读取已归档状态，但currentDataVerified=false，不把历史状态当作当前数据核验。

专项6项通过；整个workspace 511通过、0失败、177项默认忽略，格式检查、严格workspace全targets clippy及开发核心构建通过。初次缺少闭括号的编译失败日志保留，修复后完成上述检查。原有ts-rs属性解析提示仍存在。候选包检查器新增maintenanceInterlockSchema比较，13项通过，伪报该能力被拒绝；原冻结0.1.6仍可以作为原有能力候选核验，未被改写为支持新互斥。

实际Windows验证使用独立夹具、正常开发核心CLI和隐藏PowerShell FileShare.None事务锁。PowerShell PID26360持锁时占用可建立，放弃被拒绝；恢复、hold解除和核心启动也被拒绝。原锁进程明确释放并退出0后，占用仍存在，核对及明确放弃成功。反方向恢复先占用也拒绝程序reserve。原数据、原程序与hold字节保持，归档状态可读，清理通过，无待定动作；补充CIM核对无夹具非shell进程。随后用真实冷备份配合明确构造的回滚终态日志验证release，日志是合成夹具，不是实际安装或更新控制器的执行证据。无模型调用、无用户桌面动作。

109冻结归档197项及0.1.6安装包全部摘要保持。当前manage-windows-update.ps1与维护窗口尚未调用这些新命令，真实更新控制器互斥仍未验收。旧控制器的初始intent中oldEntries为空，不能直接用于reserve；后续须在初始意图中写完整原程序清单，在备份/暂存之前reserve，并在处置结束、释放其数据/事务锁后独立release，失败保留原占用。还须提供准备中断的明确放弃/终态收尾入口，并明确处理旧事务，不能改写旧intent或默认认领。Rust核对程序与数据，不代替控制器的安装登记/版本核对。

源码/debug核心版本字段暂仍0.1.6，属于未发行开发组合，旧debug桌面/sidecar未同步，不能混用。后续统一升版、能力声明、嵌入脚本和界面后构建新候选，禁止覆盖冻结0.1.6。本轮不等于实际安装、图形维护、全部崩溃/存储故障或完整P6通过。

完整P3/P4/P5/P6仍未通过，目标active。下一步接入真实程序更新控制器及恢复图形维护，再统一升版构建并补变化范围的验收。用户暂无独立Windows环境及签名证书，继续其他验收，不重复询问。原安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务，以及全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察保持。WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[110验收索引](../reports/p6-maintenance-interlock-20261012.json)；[程序更新事务](WINDOWS-UPDATE-TRANSACTION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf8')
assert '卷002第018项（109）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第019项 / 110' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第019项（110），progress，核心维护互斥与真实文件锁验证通过，更新控制器/窗口尚未接入，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方109为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第019项 / 110 核心维护互斥\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(WINDOWS-UPDATE-TRANSACTION-V1.md)', '(../../docs/WINDOWS-UPDATE-TRANSACTION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 110 核心维护互斥（控制器/发行尚未接入）\n\n' + stamp + '\n\n' + body)
note = '''\n\n110开发核心新增持久maintenance reserve/check/status/release/abandon，程序占用与恢复保护双向互斥。占用绑定完整原程序清单、原数据、SID、hold及原intent摘要，控制进程退出后仍保留。终态释放独立核验事务锁、计划/事件链、实际程序及备份；未开始准备只有原位置未变且无执行事件时才能明确放弃。release归档占用，不安装、不解除hold、不自动重放。真实PowerShell独占锁与CLI互斥已验，终态回滚日志为合成夹具。\n\n现有更新控制器和维护窗口尚未接入：初始intent须先写完整oldEntries，备份前reserve，终态在释放控制器持有的数据/事务锁后调用release；准备中断和终态收尾失败须提供明确入口，旧事务不能改写或静默认认领。安装登记/版本核查仍由控制器负责。冻结0.1.6保持，源码/debug未发行，须统一升版后构建新包。详见[110证据](../reports/p6-maintenance-interlock-20261012.json)。\n'''
note = note.replace('静默认', '默认')
for name in ['DATA-FORMAT-MIGRATION-V1.md', 'WINDOWS-UPDATE-TRANSACTION-V1.md', 'MAINTENANCE-WINDOW-V1.md']:
    with (root / 'docs' / name).open('a', encoding='utf8') as stream:
        stream.write(note)
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 110：核心持久维护互斥及真实文件锁通过；控制器/图形维护尚未接入，未发行。\n\n- 下一步接入真实更新控制器：初始完整意图、备份前reserve、显式中断处置、释放控制器锁后终态release，保留旧事务边界。\n- 再接恢复图形维护，统一升版及嵌入脚本/能力声明后构建新包，不覆盖冻结0.1.6。\n- 511项workspace通过不包含177项默认忽略；真实PowerShell锁验证不替代实际安装/更新控制器验收。\n- 暂无独立Windows环境及签名证书，继续其他验收；全部原P3/P4/P5/P6范围保持。\n- 先读[最新交接](HANDOFF-LATEST.md)与[110证据](../reports/p6-maintenance-interlock-20261012.json)。\n\n以下109及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'
text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 110）：开发核心新增程序维护与数据恢复的持久互斥，实际Windows独占锁验证通过，workspace 511项通过。更新控制器和维护窗口尚未接入，未发行；先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['crates/core/src/maintenance_interlock.rs', 'crates/core/src/backup_recovery.rs',
         'crates/core/src/lib.rs', 'crates/core/src/main.rs', 'crates/core/src/workbench_cli.rs',
         'scripts/test-windows-package.ps1', 'scripts/check-package-candidate-20261012.ps1',
         'scripts/check-maintenance-interlock-20261012.py',
         'scripts/finalize-maintenance-interlock-20261012.py', 'scripts/manage-windows-update.ps1',
         'scripts/maintenance-window.ps1', 'README.md', 'docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md', 'docs/DATA-FORMAT-MIGRATION-V1.md',
         'docs/WINDOWS-UPDATE-TRANSACTION-V1.md', 'docs/MAINTENANCE-WINDOW-V1.md',
         'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-maintenance-interlock-*-20261012.log'))
paths.update(reports.glob('p6-maintenance-interlock-*-20261012.json'))
paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '110', 'at': stamp, 'status': 'core-persistent-maintenance-interlock-and-native-lock-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False,
          'fullP5Accepted': False, 'fullP6Accepted': False, 'native': native,
          'workspace': {'passed': totals[0], 'failed': totals[1], 'ignored': totals[2]},
          'interlockTestsPassed': 6, 'candidateChecksPassed': 13, 'candidate': candidate,
          'strictClippy': 'passed', 'developmentBuild': 'passed', 'formatCheck': 'passed',
          'controllerIntegrated': False, 'maintenanceWindowIntegrated': False,
          'shippedReleaseAccepted': False, 'shippedPackageChanged': False,
          'packageManifest': package_path.relative_to(root).as_posix(),
          'packageManifestSha256': digest(package_path),
          'previousIndex': previous_path.relative_to(root).as_posix(),
          'previousIndexSha256': digest(previous_path),
          'previousArchiveRecordsVerified': len(previous['records']),
          'remaining': ['actual update controller integration and legacy transaction handling',
                        'recovery maintenance window and unified versioned release',
                        'remaining real interruption/storage faults',
                        'all original P3/P4/P5/P6 gates including installed React/clean machine/signing/providers/desktops/real management/coding/efficiency'],
          'records': records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'workspace': report['workspace'], 'goalStatus': 'active'}))
