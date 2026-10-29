"""Freeze 114: actual 0.1.7 format migration and preserved recovery round trip."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-migration-recovery-window-20261012.json'
archive = reports / 'p6-migration-recovery-window-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
prior_path = reports / 'p6-prepare-window-20261012.json'; prior = load(prior_path)
for entry in prior['records']:
    assert digest(root / entry['snapshot']) == entry['sha256']
base = load(root / prior['previousIndex'])
for entry in base['records']:
    if entry['source'].startswith(('crates/', 'apps/')) or entry['source'] in ['scripts/maintenance-window.ps1', 'scripts/manage-windows-update.ps1', 'scripts/test-windows-package.ps1', 'Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json']:
        assert digest(root / entry['source']) == entry['sha256']
package_path = root / prior['packageManifest']
assert digest(package_path) == prior['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
run = reports / 'migration-recovery-window-native-b1d6bd5726d942dd921de6035ec084a4'
first = load(run / 'result.json'); native = load(run / 'resumed-result.json')
assert first['status'] == 'failed' and not first['pendingOperation']
assert native['status'] == 'passed' and native['cleanup'] == 'passed'
assert native['calls'][0]['originalResultDigest'] == digest(run / 'result.json')
assert native['calls'][0]['noReplay'] and native['schemaRoundTrip'] == [1, 2, 1, 2]
assert native['migrationAttempts'] == 1 and native['explicitRestorationAttempts'] == 2 and native['explicitReleaseCount'] == 1
assert native['restoreSchemaOne']['exact']['exact'] and native['restoreSchemaTwo']['exact']['exact']
calls = [c for c in first['calls'] + native['calls'] if 'receipt' in c]
for c in calls:
    receipt = load(run / 'maintenance/operations' / Path(c['receipt']).name)
    assert c['exit'] == 0 and receipt['exit'] == 0 and receipt['action'] == c['action'] and receipt['phase'] == 'finished'
for action, count in [('formatMigrate', 1), ('recoveryApply', 2), ('recoveryFinish', 2), ('dataRelease', 1)]:
    assert sum(c['action'] == action for c in calls) == count
def tree(p):
    return {n.relative_to(p).as_posix(): 'directory' if n.is_dir() else digest(n) for n in p.rglob('*')}
one = Path(native['restoreSchemaOne']['archive']); two = Path(native['restoreSchemaTwo']['archive'])
assert tree(one / 'preserved') == tree(run / 'after-migration/payload')
assert tree(two / 'preserved') == tree(run / 'before-migration/payload')
marker = native['originalMigrationMarkerSha256']
assert digest(one / 'preserved/events/00000000.json') == marker
assert digest(run / 'data/events/00000000.json') == marker
assert native['restoreSchemaTwo']['format']['sourceBackupDigest'] == native['restoreSchemaOne']['manifestDigest']
config = load(run / 'maintenance/session.json')
for item in config['artifacts']:
    p = run / 'maintenance' / item['path']; assert digest(p) == item['sha256']
    if p.suffix == '.ps1': assert p.read_text(encoding='utf-8-sig') == (root / 'scripts' / p.name).read_text(encoding='utf-8-sig')
    else: assert digest(p) == digest(package_path.parent / item['path'])
cleanup = load(reports / 'p6-migration-recovery-window-cleanup-20261012.json')
assert cleanup['remainingFixtureNonShellProcesses'] == 0 and not cleanup['knownWindowPidsStillPowershell']
assert not cleanup['uninstallRegistrationPresent'] and not cleanup['productRegistrationPresent']
audit = load(reports / 'p6-migration-recovery-window-archive-audit-20261012.json'); assert audit['status'] == 'passed'
assert all(row['errors'] == 0 for row in load(reports / 'p6-migration-recovery-window-syntax-20261012.json'))
stamp = datetime.now().astimezone().isoformat()
body = '''114 使用冻结0.1.7安装与维护工具完成实际窗口格式迁移→保留恢复组合。产品、依赖和发行候选未修改。新隔离安装创建项目，窗口锁定核心后创建格式1备份，审阅并仅迁移一次到格式2；重开窗口读取格式/hold，再创建格式2备份。接着分别审阅、准备、重开读取原事务、Apply、Finish、Inspect：先从格式2恢复迁移前格式1，再恢复已备份格式2。两次原目录保留与备份发布全部由产品完成，驱动未代替移动目录或删除数据。

每次恢复在解除hold之前逐目录/文件核对：格式1清单20项，格式2清单21项，与对应备份完全相同。两份preserved原目录另用独立Python扩展路径读取核对，第一次与格式2备份相同、第二次与格式1备份相同。原格式2标记SHA256为7a91d20b39bb9a57281013f90e5c8cab4adb8d692ea54851b1697a4583ca23f6，保留和再次恢复后字节不变；其来源备份摘要仍指向最初格式1备份，不被格式2快照恢复改写。

整个组合保持同一原hold，两次恢复均核验并归档后才在窗口明确解除一次。最终安装核心在格式2下读回原项目完全一致。记录中迁移1次、recoveryApply为两个不同明确审阅的恢复共2次、finish2次、解除1次；没有用重复迁移代替恢复已迁移快照。此范围是同一维护周期内的快照往返，不等于解除后继续产生业务数据、再次锁定后任意回退旧hold备份也已支持。

保留一个测试驱动失败：首次恢复/finish/inspect均已成功，随后Get-FileHash读取超过260字符的归档路径时报不存在。独立Python和.NET扩展路径读取确认该文件存在且摘要一致，产品归档核验本身通过。修正仅测试Hash读取为扩展路径File.OpenRead+SHA256，保留修改前驱动和原失败报告。续接先核验原报告摘要、暂存工具、安装及当前格式/hold，复用仍存活且身份匹配的原窗口PID2388；不重复迁移或首次恢复，只执行尚未进行的格式2恢复。重开后的PID424完成后正常退出。AGENTS补入长路径排错规则，避免再次把提供程序错误当作文件缺失。

真实控件、操作回执及窗口显示同时核对；最后正常停止核心并卸载本次安装，CIM无夹具核心/已知窗口残留，安装登记不存在。无模型调用、无用户应用动作。维护会话仍为明确绑定隔离目录的暂存夹具，不等于安装版React→维护窗口完整入口。产品与0.1.7冻结包摘要保持，113归档57项核验通过；复用112的511项回归、177默认忽略、严格clippy/构建及14项候选检查，不无故重跑基础接入。

完整P3/P4/P5/P6未通过，目标active。下一步继续备份/暂存过程中原控制器与助手的故障处置，以及剩余真实存储故障/负载验收；物理断电、磁盘满、同步失败不由本次正常迁移往返代证。用户暂无独立Windows环境与签名证书，继续其他验收，不重复询问。安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务，以及全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察保持。WorkBuddy064不重发，不关机，不关闭用户程序，无提交推送。

[114验收索引](../reports/p6-migration-recovery-window-20261012.json)；[数据格式说明](DATA-FORMAT-MIGRATION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷003第002项（113）' in old.splitlines()[2]
volume = root / prior['handoffVolume']; assert '第003项 / 114' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷003第003项（114），progress，0.1.7实际迁移/格式1和2保留恢复组合通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方113为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as f:
    f.write('\n\n## 第003项 / 114 迁移与两格式保留恢复\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as f:
    f.write('\n\n## 114 0.1.7迁移与保留恢复往返\n\n' + stamp + '\n\n' + body)
note = '''\n\n114更新：冻结0.1.7实际窗口完成格式1→迁移格式2→恢复格式1→恢复格式2快照；仅一次迁移、两次明确恢复，产品自行保留原目录，两次恢复前后对应20/21个目录文件逐项一致，原迁移标记及来源备份摘要不变。期间保持同一hold，最后明确解除一次，项目一致。这是同一维护周期内的恢复，不等于解除后业务已变化且重新锁定时任意旧hold备份回退也被验收。独立归档核对通过；长路径测试驱动错误从原窗口续接，未重发已完成迁移/恢复。见[114证据](../reports/p6-migration-recovery-window-20261012.json)。\n'''
for name in ['MAINTENANCE-WINDOW-V1.md', 'DATA-FORMAT-MIGRATION-V1.md']:
    p = root / 'docs' / name; text = p.read_text(encoding='utf8'); heading, rest = text.split('\n', 1)
    p.write_text(heading + note + '\n以下保留历史。\n' + rest, encoding='utf8')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 114：冻结0.1.7迁移及两种格式快照的产品保留恢复组合通过。\n\n- 下一步备份/暂存中的控制器与助手联合故障处置，继续真实存储故障/负载门槛。\n- 同一hold内迁移往返已验；不要外推到重新锁定后任意历史备份回退。\n- 归档长路径读取用扩展路径File.OpenRead或Python；已完成动作只核查，不因测试失败重发。\n- 产品及冻结包未改，复用112全量回归；安装版完整React/干净机器、签名及原P3/P4/P5/P6全部范围保持。\n- 用户暂无独立环境与证书，继续其他验收。先读[最新交接](HANDOFF-LATEST.md)与[114证据](../reports/p6-migration-recovery-window-20261012.json)。\n\n以下113及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8'); start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 114）：冻结0.1.7实际窗口迁移与格式1/2快照保留恢复往返通过，目录文件、原迁移标记及项目一致。继续剩余故障验收；先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['scripts/check-migration-recovery-window-20261012.ps1', 'scripts/resume-migration-recovery-window-20261012.ps1',
         'scripts/finalize-migration-recovery-window-20261012.py', 'AGENTS.md', 'README.md', 'docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'docs/MAINTENANCE-WINDOW-V1.md', 'docs/DATA-FORMAT-MIGRATION-V1.md']
paths = {root / n for n in names} | {volume, package_path}
paths.update(reports.glob('p6-migration-recovery-window-*-20261012.*'))
paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for p in sorted(paths):
    if not p.is_file(): continue
    rel = p.relative_to(root); snapshot = archive / rel
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(p, snapshot)
    value = digest(p); assert digest(snapshot) == value
    records.append({'source': rel.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '114', 'at': stamp, 'status': 'native-format-migration-and-preserved-snapshot-round-trip-verified',
          'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
          'native': native, 'cleanup': cleanup, 'independentArchiveAudit': audit, 'productUnchanged': True,
          'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path),
          'previousIndex': prior_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(prior_path),
          'previousArchiveRecordsVerified': len(prior['records']), 'handoffVolume': volume.relative_to(root).as_posix(),
          'remaining': ['backup/staging controller and helper interruption handling', 'remaining real storage faults and load acceptance', 'all original P3/P4/P5/P6 including installed React/clean machine/signing/providers/desktops/management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as f: json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
