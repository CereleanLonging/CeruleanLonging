"""Freeze 112: versioned 0.1.7 package and actual maintenance window evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-recovery-window-20261012.json'
archive = reports / 'p6-recovery-window-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def log(p):
    b = p.read_bytes()
    return b.decode('utf-16' if b.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
prior_path = reports / 'p6-update-interlock-20261012.json'; prior = load(prior_path)
for item in prior['records']:
    assert digest(root / item['snapshot']) == item['sha256']
old_package = root / prior['packageManifest']
assert digest(old_package) == prior['packageManifestSha256']
for item in load(old_package)['artifacts']:
    assert digest(old_package.parent / item['path']) == item['sha256']
package_path = reports / 'p6-windows-package-017-20261012/manifest.json'
package = load(package_path)
assert package['version'] == '0.1.7' and not package['releaseAccepted']
assert digest(package_path) == '811bbea49bad673e43b5bb8a6c6d068483ae7a14357f38514fe2db8b26cfffca'
for item in package['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
assert package['coreBuildInfo']['dataRecoverySchema'] == 1
assert package['coreBuildInfo']['maintenanceInterlockSchema'] == 1
assert load(root / 'package.json')['version'] == '0.1.7'
assert load(root / 'apps/desktop/src-tauri/tauri.conf.json')['version'] == '0.1.7'
assert 'version = "0.1.7"' in (root / 'Cargo.toml').read_text()
for name in ['target/release/cerulean-core.exe', 'target/release/cerulean-runtime.exe',
             'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']:
    assert digest(root / name) == digest(package_path.parent / 'cerulean-runtime.exe')
rows = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', log(reports / 'p6-recovery-window-workspace-20261012.log'))
counts = [sum(int(row[i]) for row in rows) for i in range(3)]
assert counts == [511, 0, 177]
assert 'Finished `dev` profile' in log(reports / 'p6-recovery-window-clippy-20261012.log')
assert 'Finished 1 bundle at:' in log(reports / 'p6-recovery-window-package-20261012.log')
assert not log(reports / 'p6-recovery-window-fmt-20261012.log').strip()
preview = json.loads(log(reports / 'p6-recovery-window-preview-20261012.log'))
candidate = json.loads(log(reports / 'p6-recovery-window-candidate-20261012.log'))
assert preview['status'] == 'passed' and candidate['status'] == 'passed' and candidate['passed'] == 14
assert [(len(x['description']['actions']), x['description']['confirmationGateCases']) for x in preview['results']] == [(16, 9), (18, 9)]
native_path = reports / 'recovery-window-native-a30fdd86cd224b9fbe7edc066e93d3b8'
first = load(native_path / 'result.json')
middle = load(native_path / 'resumed-result.json')
last = load(native_path / 'resumed2-result.json')
assert first['status'] == 'failed' and first['calls'][-1]['action'] == 'coreReview' and first['calls'][-1]['exit'] == 4
assert middle['status'] == 'failed' and middle['calls'][-1]['action'] == 'recoveryApply' and middle['calls'][-1]['exit'] == 0
assert last['status'] == 'passed' and last['cleanup'] == 'passed'
assert middle['calls'][0]['originalResultDigest'] == digest(native_path / 'result.json')
assert last['calls'][0]['originalResultDigest'] == digest(native_path / 'resumed-result.json')
assert middle['calls'][0]['noUpdateReplay'] and last['calls'][0]['noApplyReplay']
for name in ['projectUnchanged', 'originalPreservedByProduct', 'recoveryFinishedAndInspected',
             'unstartedRecoveryAbandoned', 'installedCandidateStartupBlocked', 'reopenedTerminalFinish']:
    assert last[name]
all_calls = first['calls'] + middle['calls'] + last['calls']
receipts = [call for call in all_calls if 'receipt' in call]
for call in receipts:
    receipt = load(native_path / 'maintenance/operations' / Path(call['receipt']).name)
    assert receipt['action'] == call['action'] and receipt['exit'] == call['exit'] and receipt['phase'] == 'finished'
assert sum(c['action'] == 'Apply' for c in receipts) == 2
assert sum(c['action'] == 'Commit' for c in receipts) == 1
assert sum(c['action'] == 'recoveryApply' for c in receipts) == 1
assert sum(c['action'] == 'recoveryFinish' for c in receipts) == 1
config = load(native_path / 'maintenance/session.json')
for item in config['artifacts']:
    assert digest(native_path / 'maintenance' / item['path']) == item['sha256']
    if item['path'].endswith('.ps1'):
        assert (native_path / 'maintenance' / item['path']).read_text(encoding='utf-8-sig') == (root / 'scripts' / item['path']).read_text(encoding='utf-8-sig')
    else:
        assert item['sha256'] == digest(package_path.parent / item['path'])
cleanup = load(reports / 'p6-recovery-window-cleanup-20261012.json')
assert cleanup['remainingFixtureNonShellProcesses'] == 0 and not cleanup['knownWindowPidsStillPowershell']
assert not cleanup['uninstallRegistrationPresent'] and not cleanup['productRegistrationPresent']
stamp = datetime.now().astimezone().isoformat()
body = '''112 完成0.1.7内部候选及恢复维护窗口整合。程序窗口增加原意图读取、原维护占用摘要、FinishMaintenance和AbandonPreparation入口；字段变化取消本次确认，事务目录变化清空旧摘要。数据窗口新增恢复原hold/审阅摘要、review/status/diagnose/inspect及prepare/apply/finish/abandon入口。准备要求本窗口当前成功审阅，已存在事务可用明确原摘要读取/继续，不自动选择历史事务。所有变更逐次确认，等待沿用原进程句柄，不以超时重发；迁移/恢复后重新读取格式与hold，明确解除且不自动启动核心。原空缺目录恢复仍保留并单独标注。

Cargo、前端包/锁、Tauri统一升至0.1.7；备份写入版本白名单增加0.1.7，数据恢复能力dataRecoverySchema=1及维护互斥能力写入运行时声明。release核心、sidecar、桌面嵌入脚本和NSIS重新构建并冻结新目录，原0.1.6包保持。候选清单SHA256为811bbea49bad673e43b5bb8a6c6d068483ae7a14357f38514fe2db8b26cfffca，包仍NotSigned、releaseAccepted=false。候选14项通过，虚报恢复/互斥能力会被拒绝。workspace 511通过、0失败、177默认忽略；严格全targets clippy、格式、前端类型检查/构建和NSIS构建通过，原ts-rs属性提示仍存在。release三处核心字节一致；未重新同步整套debug运行组合，调试旧桌面/核心仍不可混用。

真实控件预览检查程序窗口16个动作/9个确认门槛，数据窗口18个动作/9个门槛通过，人工视检两个预览，布局可滚动访问下方结果区域。真实隔离0.1.6安装升级到0.1.7：通过实际窗口Apply、Rollback和再Apply，安装登记/实际版本正确；Commit时真实共享冲突只阻止最终维护占用归档，原提交已完成。窗口Review显示committed且reserved，实际安装的0.1.7核心启动被拒绝；关闭重开窗口，读取同一事务后FinishMaintenance通过，不再Commit。此次使用候选正式核心，未用开发helper覆盖。

数据窗口实际完成锁定、备份、恢复审阅与准备，关闭重开后读原事务并执行一次Apply。驱动只新增一个明确标注的待保留文件模拟原目录差异；原目录保留和备份发布均由产品完成，驱动没有代替搬移。原文件在事务preserved中可核对，恢复目录没有该额外文件。再次读状态后Finish/Inspect通过；另一个未开始的恢复准备经Diagnose、Abandon及Inspect通过。最后重新读取hold/格式、明确解除，安装核心读回原项目完全一致。

保留两个测试驱动问题及其原始失败：首段漏掉提交后启动测试核心，在线coreReview因此返回connectionFailed；修正仅测试脚本，未重复已通过的程序更新。续跑完成恢复Apply后，PowerShell Join-Path不支持返回的扩展DOS路径，断言代码失败；产品Apply已成功。核对原PID与完成回执、正常关闭本次窗口后，规范化测试路径并续接原恢复的status/finish/inspect，未重复Apply。两次续接均核验原报告摘要、暂存工具和原安装，最后报告通过；原失败文件未覆盖。最终证据组合中Apply为两个不同程序事务共2次、Commit仅1次、recoveryApply仅1次、recoveryFinish仅1次。

本轮原NSIS安装已正常卸载，登记与测试核心/已知窗口PID均无残留，无模型调用或用户应用操作。暂存维护会话是明确绑定隔离目录的夹具，窗口通过不等于安装版React→暂存→窗口完整入口；产品暂存函数精确嵌入脚本由workspace检查覆盖。程序AbandonPreparation目前新增确认门槛及111命令行证据，但该按钮的实际变更链尚未补齐；实际Prepare进程中断、剩余存储故障和全量故障矩阵仍待验。

111冻结归档144项及旧0.1.6包摘要保持。本日卷002已满20项，新增带时间戳卷003第001项。完整P3/P4/P5/P6仍未通过，目标active。下一步补程序准备中断及其窗口放弃链、新版备份/迁移/恢复组合和剩余验收，不重跑原型基础接入。用户暂无独立Windows环境及签名证书，不重复询问，继续其他验收。安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务、全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察保持。WorkBuddy064不重发，不关机，无提交推送。

[112验收索引](../reports/p6-recovery-window-20261012.json)；[0.1.7候选清单](../reports/p6-windows-package-017-20261012/manifest.json)；[维护窗口说明](MAINTENANCE-WINDOW-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第020项（111）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12' / ('handoff_2026-10-12_003_' + datetime.now().strftime('%H%M%S') + '+0800.md')
assert not volume.exists()
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷003第001项（112），progress，0.1.7候选/真实窗口更新收尾与数据恢复通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方111为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
volume.write_text('# 2026-10-12 交接卷003\n\n本卷最多20项，继承卷002及最新交接。\n\n## 第001项 / 112 0.1.7候选与恢复维护窗口\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(MAINTENANCE-WINDOW-V1.md)', '(../../docs/MAINTENANCE-WINDOW-V1.md)'), encoding='utf8')
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as f:
    f.write('\n\n## 112 0.1.7真实维护窗口（限定范围通过）\n\n' + stamp + '\n\n' + body)
note = '''\n\n112 / 0.1.7更新：窗口已整合恢复review/prepare/status/diagnose/apply/finish/abandon/inspect，允许产品保留完整原目录后在原位置恢复；旧的“需要用户先移走原目录”只适用于仍保留的空缺目录直接恢复入口。程序窗口增加原意图读取、维护占用摘要、终态FinishMaintenance和未完成准备AbandonPreparation。字段变化清除本次确认，恢复准备消耗成功审阅，其后操作需明确原审阅摘要。归档核查是历史证据，不是当前数据核验；恢复完成后仍保留hold，必须重新读取格式/hold再解除。\n\n0.1.7候选已冻结，程序真实更新/回滚、共享冲突后重开收尾、安装核心占用保护，以及产品保留/恢复数据、归档核查和未开始恢复放弃通过。安装版React到窗口完整入口仍缺独立环境，程序准备放弃按钮的实际变更链与真实Prepare中断待补；不能据窗口通过宣称全部P6通过。当前release组合一致，旧debug组合未同步。详见[112证据](../reports/p6-recovery-window-20261012.json)。\n'''
for name in ['MAINTENANCE-WINDOW-V1.md', 'DATA-FORMAT-MIGRATION-V1.md', 'WINDOWS-UPDATE-TRANSACTION-V1.md']:
    path = root / 'docs' / name
    text = path.read_text(encoding='utf8'); heading, rest = text.split('\n', 1)
    path.write_text(heading + note + '\n以下保留各批历史说明，以最新交接为准。\n' + rest, encoding='utf8')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 112：0.1.7内部候选、真实更新收尾及数据恢复窗口通过。\n\n- 下一步程序Prepare真实中断与AbandonPreparation按钮实际处置；新版迁移/恢复组合与其余故障矩阵。\n- 安装版React完整入口/干净机器待独立Windows环境；暂无环境及签名证书，按用户要求继续其他验收。\n- 冻结0.1.7已含互斥与恢复，release组合一致；旧debug组合不可混用。\n- 两次驱动错误从原事务续接，未重发Commit或recoveryApply；原失败证据保留。\n- 交接进入卷003第001项；全部原P3/P4/P5/P6范围保持。先读[最新交接](HANDOFF-LATEST.md)与[112证据](../reports/p6-recovery-window-20261012.json)。\n\n以下111及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 112）：0.1.7内部候选已冻结；真实维护窗口更新/回滚、中断收尾和产品保留恢复原数据通过。workspace 511项通过，尚有安装入口及故障矩阵等门槛；先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json', 'apps/desktop/src-tauri/tauri.conf.json',
         'apps/desktop/src-tauri/src/maintenance.rs', 'crates/core/src/lib.rs', 'crates/core/src/main.rs',
         'crates/core/src/backup_recovery.rs', 'crates/core/src/maintenance_interlock.rs',
         'scripts/maintenance-window.ps1', 'scripts/manage-windows-update.ps1', 'scripts/test-windows-package.ps1',
         'scripts/check-package-candidate-20261012.ps1', 'scripts/check-data-window-preview-20261012.ps1',
         'scripts/check-recovery-window-20261012.ps1', 'scripts/resume-recovery-window-20261012.ps1',
         'scripts/resume-recovery-finish-20261012.ps1', 'scripts/finalize-recovery-window-20261012.py',
         'scripts/desktop.mjs', 'scripts/freeze-windows-package.ps1', 'README.md', 'docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'docs/MAINTENANCE-WINDOW-V1.md',
         'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/WINDOWS-UPDATE-TRANSACTION-V1.md']
paths = {root / n for n in names} | {volume, package_path, old_package}
paths.update(reports.glob('p6-recovery-window-*-20261012.log'))
paths.update(reports.glob('p6-recovery-window-*-20261012.json'))
paths.update(p for p in native_path.rglob('*') if p.is_file())
preview_path = reports / Path(preview['run']).name
paths.update(p for p in preview_path.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '112', 'at': stamp, 'status': 'version-017-candidate-and-native-maintenance-recovery-window-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
          'workspace': {'passed': counts[0], 'failed': counts[1], 'ignored': counts[2]},
          'strictClippy': 'passed', 'packageBuild': 'passed', 'candidate': candidate, 'preview': preview,
          'native': last, 'originalNativeResult': str(native_path / 'result.json'),
          'intermediateNativeResult': str(native_path / 'resumed-result.json'), 'cleanup': cleanup,
          'uniqueProgramApplyCalls': 2, 'uniqueCommitCalls': 1, 'uniqueRecoveryApplyCalls': 1,
          'programAbandonPreparationButtonLiveAccepted': False, 'installedReactEntryAccepted': False,
          'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path),
          'previousIndex': prior_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(prior_path),
          'previousArchiveRecordsVerified': len(prior['records']), 'handoffVolume': volume.relative_to(root).as_posix(),
          'remaining': ['actual program preparation interruptions and abandonment window', 'new package migration/recovery combinations and remaining storage faults', 'all original P3/P4/P5/P6 requirements including installed React/clean machine/signing/providers/desktops/management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as f: json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'handoff': str(volume), 'goalStatus': 'active'}))
