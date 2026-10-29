"""Freeze 115 partial evidence; never infer process cleanup from a backup file."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-backup-interrupt-pending-20261012.json'
archive = reports / 'p6-backup-interrupt-pending-sources-20261012'
assert not index.exists() and not archive.exists()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def digest(p): return sha256(p.read_bytes()).hexdigest()
previous_path = reports / 'p6-migration-recovery-window-20261012.json'; previous = load(previous_path)
for item in previous['records']:
    assert digest(root / item['snapshot']) == item['sha256']
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
run = reports / 'backup-interrupt-window-native-9340188633364a59bf035f2df6d250e7'
native = load(run / 'result.json'); observation = load(reports / 'p6-backup-interrupt-filesystem-observation-20261012.json')
assert native['status'] == 'failed' and native['cleanup'] == 'preserved-for-inspection'
assert native['calls'][-1]['action'] == 'Prepare' and native['calls'][-1]['exit'] == -1
assert observation['allManifestEntriesMatch'] and observation['backupManifestEntryCount'] == 21
byte_audit = load(reports / 'p6-backup-interrupt-original-bytes2-20261012.json')
assert byte_audit['dataFilesMatchPretest'] and byte_audit['programFilesMatchPretest']
syntax = load(reports / 'p6-backup-interrupt-driver-syntax-20261012.json')
assert syntax['errors'] == 0 and not syntax['executionRetested']
config = load(run / 'maintenance/session.json')
for item in config['artifacts']:
    p = run / 'maintenance' / item['path']; assert digest(p) == item['sha256']
    if p.suffix == '.ps1': assert p.read_text(encoding='utf-8-sig') == (root / 'scripts' / p.name).read_text(encoding='utf-8-sig')
    else: assert digest(p) == digest(package_path.parent / item['path'])
stamp = datetime.now().astimezone().isoformat()
body = '''115 当前是未完成的备份中断验收，不能记为通过。新驱动使用冻结0.1.7维护工具和隔离0.1.6安装，256MiB明确标注原数据夹具，仅发起一次Prepare，计划在备份payload创建时核验助手父子关系、可执行路径和事务后终止控制器。窗口PID22348，控制器PID5552；原进程句柄捕获记录与窗口退出回执存在，Prepare回执exit=-1。随后验证驱动读取stderr遇到共享占用，提前进入失败收尾，尚未保存完整watcher观察结果，也没有执行AbandonPreparation或卸载。

上次正常宿主CIM只读进程核查被自动审批服务拒绝执行：原因是额度限制导致审批无法完成，并非安全性判定。没有改走另一权限路径绕过审批。后续原工具session61424已无法查询（Unknown process id），只能说明该工具句柄不可用，不能据此断言窗口/助手已退出。当前进程及安装登记未重新核查，测试安装/维护占用按未清理处理；本批没有终止其他进程、解锁、删除或重做Prepare。Helper故障模式尚未执行。

在允许的工作区内只读检查：原事务目录已出现data-backup、intent.json、maintenance-reservation.json及transaction.lock；备份清单21项存在且目录/文件大小/摘要匹配，原hold和同级维护占用仍在。对测试前清单逐文件核对，原数据与原程序字节保持。第一份比较错误地把计算出的十六进制转大写而原清单为小写，产生两个false；已保留该失败文件，并在第二份审计中统一大小写后核对通过。此为审计脚本问题，不是原文件发生变化。静态备份完整性和原字节保持不能证明进程已退出，也不能证明整个控制器/助手中断场景已验收。

保留原测试驱动，修正新驱动：收到父控制器退出回执后不立即清空pendingOperation；预期失败路径先等待持有原句柄的watcher确认助手生命周期，超时或错误保留待定、不重启；无论日志/界面验证在哪一步失败，finally都会独立保存watcher快照，done=false/helperHasExited=false仍明确未决。显示核查成功之后才清除驱动pending状态。修改仅语法检查通过，未实机复测；产品maintenance-window.ps1、控制器、核心及冻结0.1.7包未改变。原产品窗口仍在结束回执后尝试读日志；是否同样出现读取竞争、是否需要产品修复，须先查原窗口实际状态，不能只凭驱动异常直接定论或声称已修好。

114归档180项及0.1.7包摘要保持。冻结本批已能读取的原报告、回执、请求、修改前后驱动和本地审计；不复制可能仍写入的原日志或整个活动数据目录作为已稳定证据。完整P3/P4/P5/P6仍未通过，目标active；本轮分类为progress（新增静态证据、修复驱动待定状态与保存交接），不是完成或阻塞终局。

恢复审批服务后第一步必须重新核对这个原现场：以PID、父PID、完整脚本/事务路径和原句柄信息判定身份，核查原备份助手、窗口及原安装登记。PID可能重用，不能仅凭数字关闭。只读取同一事务与日志；如仍有助手，等待原助手，不能重启或先解除占用。确认原生命周期与数据/程序/hold后才明确放弃该未开始准备并清理本次安装。若窗口日志显示存在产品缺口，做最小修复和新版本打包，禁止覆盖冻结0.1.7。原Controller场景完成审阅处置后，再决定是否需要新场景；Helper模式仍待验，不能在原安装未清理时直接新建安装。

用户暂无独立Windows环境及签名证书的决定保持，不重复询问。原所有安装版React/干净机器、统一程序数据事务、跨用户路径、剩余存储/负载故障、首批供应商/CLI/Agent/两桌面、真实管理/编程/效率、金额预算/按执行者监督和075/085门槛保留。WorkBuddy064不重发，不关机，无提交推送。

[115未决证据](../reports/p6-backup-interrupt-pending-20261012.json)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷003第003项（114）' in old.splitlines()[2]
volume = root / previous['handoffVolume']; assert '第004项 / 115' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷003第004项（115），progress，备份中断实机验收未决；原测试安装未确认清理，须先查原现场，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方114为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as f: f.write('\n\n## 第004项 / 115 备份中断未决现场\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as f: f.write('\n\n## 115 备份中断待核查（未通过）\n\n' + stamp + '\n\n' + body)
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 115：有未确认清理的原测试安装，备份中断未通过。\n\n- 首先读最新交接，恢复自动审批后核查原PID22348/5552及事务backup-interrupt-window-native-9340188633364a59bf035f2df6d250e7；核验身份防PID重用。\n- 不重发Prepare，不新装，不从文件完整或工具session消失推断助手退出。先核查原日志/原进程，再处置原事务。\n- 原备份21项静态匹配，原数据/程序字节保持；进程生命周期、放弃/清理、窗口显示仍未决。驱动修正仅语法通过，产品未改。\n- Helper模式、剩余故障及原P3/P4/P5/P6全部门槛保持；无独立环境/证书继续其他可做工作。\n- 详见[最新交接](HANDOFF-LATEST.md)和[115未决证据](../reports/p6-backup-interrupt-pending-20261012.json)。\n\n以下114及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; s = readme.read_text(encoding='utf8'); start = s.index('最新进展（'); end = s.index('\n\n', start)
readme.write_text(s[:start] + '最新进展（2026-10-12 / 115）：备份中断验收遇到日志共享占用，原测试安装清理未确认。静态备份与原数据/程序字节核对通过；先查[最新交接](docs/HANDOFF-LATEST.md)恢复原现场，不重发Prepare或新建安装。完整P0–P6尚未通过。' + s[end:], encoding='utf8')
names = ['scripts/check-backup-interrupt-window-20261012.ps1', 'scripts/finalize-backup-interrupt-pending-20261012.py', 'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path, run / 'result.json', run / 'owned-prepare-process.json', run / 'original-data-inventory.txt', run / 'original-program-inventory.txt'}
paths.update(reports.glob('p6-backup-interrupt-*-20261012.json'))
paths.add(reports / 'p6-backup-interrupt-driver-before-20261012.ps1')
paths.update((run / 'maintenance/operations').glob('*.request.json'))
paths.update((run / 'maintenance/operations').glob('*.finished.json'))
archive.mkdir(); records = []
for p in sorted(paths):
    rel = p.relative_to(root); target = archive / rel
    target.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(p, target)
    value = digest(p); assert digest(target) == value
    records.append({'source': rel.as_posix(), 'snapshot': target.relative_to(root).as_posix(), 'sha256': value})
report = {'batch':'115','at':stamp,'status':'backup-interruption-evidence-pending-process-review','goalStatus':'active',
          'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
          'nativeFailedResult':native,'filesystemObservation':observation,'originalByteAudit':byte_audit,
          'processLivenessVerified':False,'cleanupVerified':False,'driverFixLiveVerified':False,'helperFaultModeExecuted':False,
          'approvalReviewIssue':'usage-limit review failure; no unsafe determination; original CIM action not executed',
          'packageManifest':package_path.relative_to(root).as_posix(),'packageManifestSha256':digest(package_path),
          'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),
          'previousArchiveRecordsVerified':len(previous['records']),'handoffVolume':volume.relative_to(root).as_posix(),
          'remaining':['original fixture process/log audit then explicit abandonment and cleanup','driver fix live verification and possible product log handling gap','helper interruption and all remaining original P3/P4/P5/P6 gates'],'records':records}
with index.open('x',encoding='utf8') as f: json.dump(report,f,ensure_ascii=False,indent=2); f.write('\n')
print(json.dumps({'batch':'115','records':len(records),'cleanupVerified':False,'goalStatus':'active'}))
