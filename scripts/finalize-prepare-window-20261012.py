"""Freeze 113: actual preparation process interruption and window abandonment."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-prepare-window-20261012.json'
archive = reports / 'p6-prepare-window-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def log(p):
    b = p.read_bytes()
    return b.decode('utf-16' if b.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
prior_path = reports / 'p6-recovery-window-20261012.json'; prior = load(prior_path)
for record in prior['records']:
    assert digest(root / record['snapshot']) == record['sha256']
    if record['source'].startswith(('crates/', 'apps/')) or record['source'] in ['scripts/maintenance-window.ps1', 'scripts/manage-windows-update.ps1', 'scripts/test-windows-package.ps1', 'Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json']:
        assert digest(root / record['source']) == record['sha256']
package_path = root / prior['packageManifest']; package = load(package_path)
assert digest(package_path) == prior['packageManifestSha256']
for item in package['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
native = json.loads(log(reports / 'p6-prepare-window-native1-20261012.log'))
run = reports / Path(native['run']).name
assert load(run / 'result.json') == native
assert native['status'] == 'passed' and native['cleanup'] == 'passed'
assert native['prepareAttempts'] == 1 and not native['testCheckpointInProduct']
fault = load(run / 'prepare-termination.json')
owner = load(run / 'owned-prepare-process.json')
assert fault == native['realControllerTermination']
assert fault['wasAlive'] and fault['hasExited'] and fault['exit'] != 0 and not fault['error']
assert owner['handleCaptured'] and owner['pid'] == fault['pid']
assert native['helperObservations'][-1]['pids'] == []
for key in ['originalDataAndProgramUnchanged', 'partialTransactionEvidencePreserved', 'actualAbandonPreparationButton', 'reopenedArchivedReadback', 'originalHoldPreservedUntilExplicitRelease', 'projectUnchanged']:
    assert native[key]
calls = [c for c in native['calls'] if 'receipt' in c]
assert sum(c['action'] == 'Prepare' for c in calls) == 1
assert sum(c['action'] == 'AbandonPreparation' for c in calls) == 2
for c in calls:
    receipt = load(run / 'maintenance/operations' / Path(c['receipt']).name)
    assert receipt['phase'] == 'finished' and receipt['action'] == c['action'] and receipt['exit'] == c['exit']
transaction = run / 'transaction-interrupted-prepare'
assert sorted(p.name for p in transaction.iterdir()) == ['intent.json', 'maintenance-reservation.json', 'transaction.lock']
config = load(run / 'maintenance/session.json')
for item in config['artifacts']:
    p = run / 'maintenance' / item['path']; assert digest(p) == item['sha256']
    if p.suffix == '.ps1':
        assert p.read_text(encoding='utf-8-sig') == (root / 'scripts' / p.name).read_text(encoding='utf-8-sig')
    else:
        assert digest(p) == digest(package_path.parent / item['path'])
cleanup = load(reports / 'p6-prepare-window-cleanup-20261012.json')
assert cleanup['remainingFixtureNonShellProcesses'] == 0 and not cleanup['knownWindowAndControllerPidsStillPowershell']
assert not cleanup['uninstallRegistrationPresent'] and not cleanup['productRegistrationPresent']
stamp = datetime.now().astimezone().isoformat()
body = '''113 补齐冻结0.1.7维护窗口的程序准备中断与AbandonPreparation实际变更链。产品、依赖、版本、安装包均未改动，复用112的核心/维护脚本与真实控件驱动。新隔离0.1.6安装创建项目并锁定，加入明确标注的64MiB原数据夹具，再用0.1.7窗口仅点击一次Prepare。测试监视器观察真实maintenance-reservation.json发布，通过父窗口PID、脚本路径和事务目录捕获原控制器句柄后终止该控制器。没有向产品添加停点、延迟或伪造执行事件。

原控制器PID18220，父窗口PID20256；观察时确实存活，原句柄等待确认退出-1，窗口显示原失败及退出回执。之后实际查询命令行关联该事务的核心助手，没有仍存活的匹配助手；不把父进程退出当作子进程已退出。本次终止发生在备份启动前，最终准备目录只有intent.json、maintenance-reservation.json和transaction.lock，无备份、plan或事件；不能据此宣称备份中途或发布后所有中断点通过。

原数据与原程序文件清单/摘要保持。正常关闭并重开窗口（PID6208），读取原意图摘要、明确填入原占用摘要与包信任策略，再点击“放弃未完成准备”：只归档维护占用，原hold仍在，原程序、原数据及三个残留事务文件摘要全部不变。再次关闭重开（PID2964），同一意图/占用下再次明确核查放弃入口，读取released历史状态，不重新Prepare，不改残留证据或数据。两个放弃入口调用对应首次归档与归档后核查，Prepare只有一次。

随后明确解除原hold、启动原安装核心读回原项目一致，停止本次核心并卸载测试安装。清理通过，CIM无本次非shell进程，已知窗口/控制器PID均不再是PowerShell，安装登记不存在。没有模型调用、用户应用操作或关机。安装版React完整入口仍未由此覆盖，暂存会话仍是明确绑定隔离目录的夹具。

112冻结归档280项、相关产品源码/依赖及0.1.7安装包全部摘要保持，继承112的511项workspace、177默认忽略、严格clippy、构建与14项候选检查，不重复未变的基础接入、正常升级或整套回归。新增测试驱动语法检查通过，原证据及新报告单独冻结。程序准备放弃按钮的实际变更链已补齐；磁盘满、同步落盘失败、物理断电、备份/暂存过程中控制器与助手的联合恢复等剩余范围不由本例代证。

完整P3/P4/P5/P6仍未通过，目标active。下一步验证新版迁移→产品保留恢复组合并继续其余真实中断/存储故障。用户暂无独立Windows和代码签名证书，不重复询问，继续其他验收。安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务，以及全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察保持。WorkBuddy064不重发，不关闭用户程序，无提交推送。

[113验收索引](../reports/p6-prepare-window-20261012.json)；[维护窗口说明](MAINTENANCE-WINDOW-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷003第001项（112）' in old.splitlines()[2]
volume = root / prior['handoffVolume']; assert '第002项 / 113' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷003第002项（113），progress，真实Prepare中断与窗口放弃/重开核查通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方112为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as f:
    f.write('\n\n## 第002项 / 113 准备中断与窗口放弃\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(MAINTENANCE-WINDOW-V1.md)', '(../../docs/MAINTENANCE-WINDOW-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as f:
    f.write('\n\n## 113 实际准备控制器中断（限定范围通过）\n\n' + stamp + '\n\n' + body)
note = '''\n\n113更新：冻结0.1.7窗口Prepare在真实占用回执发布后被终止，原控制器句柄确认退出-1，备份尚未启动。重开窗口读取原意图并明确AbandonPreparation，原hold、原程序/数据及残留事务文件保持；再次重开核查已归档状态不重复准备。程序准备放弃按钮的实际变更链已验，暂存会话仍是隔离夹具，非完整安装版React入口。备份中途、磁盘满、同步失败及物理断电仍缺独立证据，见[113验收](../reports/p6-prepare-window-20261012.json)。\n'''
for name in ['MAINTENANCE-WINDOW-V1.md', 'WINDOWS-UPDATE-TRANSACTION-V1.md']:
    p = root / 'docs' / name; text = p.read_text(encoding='utf8'); heading, rest = text.split('\n', 1)
    p.write_text(heading + note + '\n以下保留历史。\n' + rest, encoding='utf8')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 113：真实Prepare控制器中断、程序准备放弃窗口及重开核查通过。\n\n- 下一步新版迁移→产品保留恢复组合，再继续备份/暂存中断、磁盘满/同步故障等缺口。\n- 本次中断在备份前，占用记录已发布；不能代证仍有助手工作时的全部处置。\n- 产品及0.1.7冻结包未改，复用112构建和核心回归；程序AbandonPreparation按钮实际链已补齐。\n- 完整安装版React入口/干净机器、签名及全部原P3/P4/P5/P6门槛保持；暂无环境/证书，继续其他验收。\n- 先读[最新交接](HANDOFF-LATEST.md)与[113证据](../reports/p6-prepare-window-20261012.json)。\n\n以下112及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 113）：冻结0.1.7维护窗口的真实Prepare中断、明确放弃及重开核查通过，原程序/数据/hold和残留证据保持。继续新版迁移恢复及剩余故障验收；先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['scripts/check-prepare-window-20261012.ps1', 'scripts/finalize-prepare-window-20261012.py',
         'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md',
         'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'docs/MAINTENANCE-WINDOW-V1.md', 'docs/WINDOWS-UPDATE-TRANSACTION-V1.md']
paths = {root / n for n in names} | {volume, package_path}
paths.update(reports.glob('p6-prepare-window-*-20261012.log'))
paths.update(reports.glob('p6-prepare-window-*-20261012.json'))
paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for p in sorted(paths):
    rel = p.relative_to(root); snapshot = archive / rel
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(p, snapshot)
    value = digest(p); assert digest(snapshot) == value
    records.append({'source': rel.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '113', 'at': stamp, 'status': 'real-prepare-interruption-and-window-abandonment-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
          'native': native, 'cleanup': cleanup, 'productUnchanged': True, 'packageChanged': False,
          'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path),
          'previousIndex': prior_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(prior_path),
          'previousArchiveRecordsVerified': len(prior['records']), 'handoffVolume': volume.relative_to(root).as_posix(),
          'remaining': ['new package format migration and product-preserving restoration combination', 'remaining backup/staging interruptions and real storage faults', 'all original P3/P4/P5/P6 gates including installed React/clean machine/signing/providers/desktops/management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as f: json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
