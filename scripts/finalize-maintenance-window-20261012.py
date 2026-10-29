"""Freeze batch 100; retain previous evidence and all unaccepted full-stage gates."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging')
reports = root / 'reports'
index = reports / 'p6-maintenance-window-20261012.json'
archive = reports / 'p6-maintenance-window-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))

def log(name):
    raw = (reports / name).read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

def result(name):
    return json.loads([line for line in log(name).splitlines() if line.startswith('{')][-1])

previous_path = reports / 'p6-commit-recovery-20261012.json'
previous = load(previous_path)
for entry in previous['records']:
    assert digest(root / entry['snapshot']) == entry['sha256'], entry['snapshot']
    if entry['source'] in ('scripts/manage-windows-update.ps1', 'scripts/test-windows-package.ps1'):
        assert digest(root / entry['source']) == entry['sha256']
old_package = root / previous['packageManifest']
assert digest(old_package) == previous['packageManifestSha256']
for entry in load(old_package)['artifacts']:
    assert digest(old_package.parent / entry['path']) == entry['sha256']

native = result('p6-maintenance-native2-20261012.log')
react = result('p6-maintenance-react-20261012.log')
window = result('p6-maintenance-window-probe4-20261012.log')
installer = result('p6-maintenance-installer-20261012.log')
assert native['status'] == react['status'] == installer['status'] == 'passed'
assert native['maintenanceWindowClosed'] and native['noMaintenanceActionsOnOpen'] and native['coreNotStopped']
assert react['passed'] == 6 and window['confirmationGateCases'] == 7 and window['automaticActions'] == 0
assert '2 passed; 0 failed' in log('p6-maintenance-stage-unit5-20261012.log')
assert 'Finished' in log('p6-maintenance-clippy3-20261012.log')
assert '8 passed; 0 failed' in log('p6-maintenance-backup-20261012.log')
assert 'Finished' in log('p6-maintenance-package-build-20261012.log')
package_path = reports / 'p6-windows-package-013-20261012/manifest.json'
package = load(package_path)
assert package['version'] == '0.1.3' and not package['releaseAccepted']
for entry in package['artifacts']:
    assert digest(package_path.parent / entry['path']) == entry['sha256']
runtime_paths = ['target/release/cerulean-core.exe', 'target/release/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime_hashes = {name: digest(root / name) for name in runtime_paths}
assert len(set(runtime_hashes.values())) == 1
session = root / Path(native['session']['directory']).relative_to(Path(r'D:\test\CeruleanLonging'))
assert digest(session / 'session.json') == native['session']['sessionDigest']
for entry in load(session / 'session.json')['artifacts']:
    assert digest(session / entry['path']) == entry['sha256']

stamp = datetime.now().astimezone().isoformat()
body = '''100 将维护工具嵌入0.1.3桌面程序，新增“维护与升级”页和独立WinForms窗口。Rust在安装/数据之外生成唯一会话副本，保存SID、版本、摘要，固定三个嵌入脚本并复制核心；窗口核验后持有原文件句柄。打开仅准备工具，不升级、不停止核心。操作仍调用099原事务，底层脚本摘要保持；版本和备份writer白名单推进至0.1.3，活动数据格式未变。

界面支持核心审阅/锁定、包清单读取、准备/审阅/更新/回滚/提交/对账/放弃。每次变更单独确认，子进程运行时禁用输入、阻止窗口关闭，保存原请求及输出/退出记录，不超时重发。替换程序前用户关闭原工作台，独立窗口保持；主窗口返回PID仅代表进程创建，尚无加载就绪ACK。维护宿主异常退出后仍须核对子进程生命周期，不能凭无完成记录重发。

React六项使用模拟Tauri；Rust两项暂存/重叠/硬链接检查；实际WinForms十二按钮、七项确认门槛无外部操作通过，最终预览已检查。实际隔离Tauri/WebView2启动独立窗口通过，三脚本及核心副本摘要相符，原核心PID保持、无hold/operations，测试窗口正常关闭。严格workspace全targets clippy、八项备份专项、完整前端/release/NSIS构建通过。0.1.3实际隔离安装payload摘要匹配，活动核心时重装/卸载拒绝且PID与文件保持，停止本次测试核心后卸载并清理登记，数据未变；不是干净机器验收。

保留失败：PS5将扩展DOS路径当远程脚本，转换为普通路径后保持RemoteSigned通过；首个实机窗口继承模块路径导致Get-FileHash不可用，给子进程指定系统PS5模块目录后通过，原错误窗口已按PID/会话归属正常关闭。一次检查因当前PATH缺cargo未运行，补充当前进程PATH后通过，不记为检查成功。初次不可见预览空白保留，后续无激活的实际控件绘制通过。

本批没有通过全套维护窗口按钮执行升级/回滚；099底层事务通过作为继承证据，不能代证UI完整操作链。0.1.3三项包仍未签名、releaseAccepted=false；release三处runtime一致，debug也是0.1.3但不同构建。099冻结归档及0.1.2包核对保持，旧基础模型接入不重复，真实模型/用户桌面动作0，WorkBuddy064不重发。

完整P3/P4/P5/P6仍未通过，目标active。下一步补完整窗口操作链与恢复、活动数据格式门槛/迁移、其余真实进程/磁盘满/断电、正式签名/更新服务/干净目标。保留全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察。证书身份问题仍待答复；不关机、不关闭用户程序、无提交推送。

[维护窗口](MAINTENANCE-WINDOW-V1.md)；[100验收索引](../reports/p6-maintenance-window-20261012.json)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf8')
assert '卷002第008项（099）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第009项（100），progress，维护组件分发与独立窗口入口通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方099为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第009项 / 100' not in volume.read_text(encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第009项 / 100 维护组件分发与独立窗口\n\n' + stamp + '\n\n' + body.replace('(MAINTENANCE-WINDOW-V1.md)', '(../../docs/MAINTENANCE-WINDOW-V1.md)').replace('(../reports/', '(../../reports/'))
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md'
with matrix.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 100 维护窗口（限定范围通过）\n\n' + stamp + '\n\n' + body)
gates = root / 'docs/NEXT-STAGE-GATES.md'
old_gates = gates.read_text(encoding='utf8')
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 100：0.1.3维护组件已嵌入并随内部安装包分发，桌面打开独立窗口与确认门槛通过。完整P3/P4/P5/P6未通过。详见[维护窗口](MAINTENANCE-WINDOW-V1.md)与[最新交接](HANDOFF-LATEST.md)。\n\n- 下一步补完整窗口升级/回滚/恢复操作链；原099底层事务通过继续复用。\n- 活动数据格式门槛/迁移、其余故障窗口、签名/更新服务与干净目标仍待验收。\n- 所有原首批接入、真实主管/编程/效率、两桌面和075/085等缺口保持；不重发WorkBuddy064。\n- 证书身份问题待答复。debug与release同为0.1.3，但不是相同构建。\n\n以下099及更早内容为历史，后续步骤以100为准。\n\n' + old_gates.removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'
text = readme.read_text(encoding='utf8')
start = text.index('最新进展（'); end = text.index('\n\n', start)
text = text[:start] + '最新进展（2026-10-12 / 100）：0.1.3已加入维护入口，内嵌工具可从桌面打开独立维护窗口；确认门槛、实机打开、内部安装包隔离安装/卸载通过。尚缺完整窗口升级操作链、活动数据迁移及正式发布等验收。先读[最新交接](docs/HANDOFF-LATEST.md)与[维护窗口](docs/MAINTENANCE-WINDOW-V1.md)。完整P0–P6尚未通过。' + text[end:]
text = text.replace('当前不制作安装包。', '可用 `npm.cmd run desktop:package` 生成内部NSIS安装包；冻结包与签名限制见最新交接。')
readme.write_text(text, encoding='utf8')
transaction = root / 'docs/WINDOWS-UPDATE-TRANSACTION-V1.md'
text = transaction.read_text(encoding='utf8')
text = text.replace('# Windows 程序更新事务\n\n', '# Windows 程序更新事务\n\n100 更新：0.1.3已内嵌三个维护脚本并提供独立窗口入口，实机打开及确认门槛通过。完整窗口升级操作链仍待验收；下文098/099分发状态为历史。见[维护窗口](MAINTENANCE-WINDOW-V1.md)。\n\n', 1)
transaction.write_text(text, encoding='utf8')

names = ['Cargo.toml','Cargo.lock','package.json','package-lock.json','README.md','apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/src/main.rs','apps/desktop/src-tauri/src/maintenance.rs','apps/desktop/src/App.tsx','apps/desktop/src/Maintenance.tsx','crates/core/src/lib.rs','crates/core/src/backup.rs','scripts/maintenance-window.ps1','scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1','scripts/finalize-maintenance-window-20261012.py','tests/maintenance-ui.mjs','tests/maintenance-native.mjs','docs/MAINTENANCE-WINDOW-V1.md','docs/WINDOWS-UPDATE-TRANSACTION-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-maintenance-*-20261012.log'))
for value in (native, react, installer):
    run = reports / Path(value['run']).name
    paths.add(run / 'result.json')
    paths.update(run.glob('*.png'))
paths.update(session.glob('*.ps1')); paths.update(session.glob('*.json')); paths.update(session.glob('*.png'))
failed = reports / 'maintenance-native-p4qYR8'
paths.update(failed.glob('maintenance-cache/*/startup-error.txt'))
records = []
archive.mkdir()
for path in sorted(paths):
    relative = path.relative_to(root); target = archive / relative
    target.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, target)
    value = digest(path); assert digest(target) == value
    records.append({'source':relative.as_posix(),'snapshot':target.relative_to(root).as_posix(),'sha256':value})
report = {'batch':'100','at':stamp,'status':'maintenance-distribution-entry-and-confirmation-gates-verified','goalStatus':'active','fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'realModelCalls':0,'userDesktopActions':0,'results':{'native':native,'react':react,'window':window,'installer':installer},'fullMaintenanceUiWorkflowAccepted':False,'packageManifest':package_path.relative_to(root).as_posix(),'packageManifestSha256':digest(package_path),'releaseRuntimeHashes':runtime_hashes,'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'previousArchiveRecordsVerified':len(previous['records']),'previousPackageUnchanged':True,'remaining':['complete native UI update/rollback/recovery workflow','active data format gates/migration','remaining process/crash/disk-full/power-loss matrix','signing/update service/clean target','all original real-provider/desktop/management/coding/efficiency scope'],'archiveScope':'selected source and evidence; not complete reproducible repository','records':records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'goalStatus':'active'}, ensure_ascii=False))
