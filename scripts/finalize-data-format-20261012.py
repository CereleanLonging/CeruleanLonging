"""Freeze batch 102: explicit container migration and version readability gates."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging')
reports = root / 'reports'
index = reports / 'p6-data-format-20261012.json'
archive = reports / 'p6-data-format-sources-20261012'
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

previous_path = reports / 'p6-maintenance-flow-20261012.json'
previous = load(previous_path)
for entry in previous['records']:
    assert digest(root / entry['snapshot']) == entry['sha256'], entry['snapshot']
old_package = root / previous['packageManifest']
assert digest(old_package) == previous['packageManifestSha256']
for entry in load(old_package)['artifacts']:
    assert digest(old_package.parent / entry['path']) == entry['sha256']

results = {key: result(name) for key, name in {
    'nativeMigration': 'p6-data-format-native1-20261012.log',
    'candidate': 'p6-data-format-candidate-20261012.log',
    'updateGate': 'p6-data-format-update-gate-20261012.log',
    'maintenanceWindow': 'p6-data-format-window-20261012.log',
}.items()}
for key, value in results.items():
    run = reports / Path(value['run']).name
    assert value['status'] == 'passed', key
    if key != 'candidate':
        assert load(run / 'result.json') == value, key
    assert value['realModelCalls'] == 0 and value['userDesktopActions'] == 0
native = results['nativeMigration']
for key in ['explicitMigration', 'wrongReviewNoMutation', 'originalFileHashesPreserved',
            'approvedPlansPreserved', 'projectWorkflowPreserved', 'legacyReaderRejected',
            'futureSchemaRejectedBeforeMutation', 'repeatMigrationRejected',
            'originalLocationRestoreExact', 'legacyReaderWorksAfterRestore',
            'controlledPartialPublicationBlocksRelease', 'holdReleaseExplicit']:
    assert native[key], key
assert native['cleanup'] == 'passed' and not native['realCrashTested']
assert results['candidate']['passed'] == 12
assert results['updateGate']['passed'] == 6
window = results['maintenanceWindow']
for key in ['currentTools', 'actualWinFormsControls', 'displayedResultsVerified',
            'stagingFixture', 'windowReopenedBeforeRollback', 'commit', 'projectUnchanged']:
    assert window[key], key
assert window['cleanup'] == 'passed' and not window['productDesktopLauncherRetested']
assert window['sourceVersion'] == '0.1.4' and window['candidateVersion'] == '0.1.5'
assert sum(c['action'] == 'Apply' for c in window['calls']) == 2
assert sum(c['action'] == 'Rollback' for c in window['calls']) == 1
assert sum(c['action'] == 'Commit' for c in window['calls']) == 1
assert '3 passed; 0 failed' in log('p6-data-format-unit2-20261012.log')
assert 'Finished' in log('p6-data-format-clippy1-20261012.log')
assert 'Finished 1 bundle' in log('p6-data-format-package-build-20261012.log')
counts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', log('p6-data-format-workspace-20261012.log'))
totals = [sum(int(row[i]) for row in counts) for i in range(3)]
assert totals == [489, 0, 174], totals

package_path = reports / 'p6-windows-package-015-20261012/manifest.json'
package = load(package_path)
assert package['version'] == '0.1.5' and not package['releaseAccepted']
assert package['coreBuildInfo']['readableDataSchemas'] == [1, 2]
for entry in package['artifacts']:
    assert digest(package_path.parent / entry['path']) == entry['sha256']
    assert entry['authenticodeStatus'] == 'NotSigned'
image = (package_path.parent / 'CeruleanLonging.exe').read_bytes()
for name in ['maintenance-window.ps1', 'manage-windows-update.ps1', 'test-windows-package.ps1']:
    assert (root / 'scripts' / name).read_bytes() in image, name
runtime_names = ['target/release/cerulean-core.exe', 'target/release/cerulean-runtime.exe',
                 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime_hashes = {name: digest(root / name) for name in runtime_names}
assert set(runtime_hashes.values()) == {package['coreSha256']}

stamp = datetime.now().astimezone().isoformat()
body = '''102 补齐活动数据容器格式门槛及显式1→2迁移。0.1.5默认新建目录仍为格式1，启动不自动迁移。格式2在events/00000000.json保存严格的格式、读取器代际、原路径/SID和来源备份摘要；业务事件、顺序、哈希及批准字节不改写。新核心在加载前和取得写锁后核对标记，旧0.1.4读取器会在构建执行宿主前拒绝不认识的结构。不是业务事件字段重写，也不宣称阻断旧IPC客户端连接兼容的新服务。

迁移CLI要求原停机hold、完整备份及迁移审阅摘要，核对备份与原数据逐项一致，在原写锁和文件句柄保持期间发布唯一临时文件。未知/损坏标记、错误摘要、数据变化、活动写锁、重复迁移拒绝；未完成标记阻止启动和新版解除hold。迁移保持锁定，后续显式解除；响应丢失先读取现状，不自动重做。程序更新事务中间不插入数据迁移，先完成同格式程序更新，再单独审阅、锁定、备份和迁移。

实机使用冻结0.1.4读取器和0.1.5 debug核心：原计划、批准与项目保持，原文件摘要未变，旧程序拒绝格式2且未改数据；保留整个已迁移目录后，在原路径从备份恢复，文件摘要完全一致，旧程序重新读取通过。未来格式与受控残缺发布拒绝；残缺发布是明确夹具，不代证真实断电、磁盘满或迁移进程终止。测试核心清理通过，无真实模型/用户桌面动作。

候选检查器核对实际运行时的数据门槛和可读格式声明，12项通过；更新工具要求源/候选都能读取活动格式，6项受控矩阵通过，含真实新版核心读取。不能将同一0.1.5能力组合视作两个新版之间已做格式2程序替换。完整workspace489通过、174项忽略、0失败；新增格式专项3项、格式检查及严格clippy通过。首次编译的3处IO错误转换失败保留，修复后通过。

0.1.5前端/release/NSIS构建与冻结通过，三个维护脚本原字节均嵌入GUI，release三处runtime一致。使用所有当前维护工具，实际窗口完成0.1.4安装到0.1.5更新、关闭重开后回滚、再更新并提交，显示与回执一致，原项目保持，测试安装及登记清理通过。沿用100暂存/启动机制，专项仍为明确组装的隔离暂存夹具，不宣称整条安装版React入口已验收。未重新进行0.1.5独立干净安装；debug核心已为0.1.5，debug桌面及runtime仍是100旧构建，不能混用。

101冻结归档274项及0.1.4包摘要保持，旧接入证据继续继承，不重复基础接入。用户已确认无Windows签名证书，继续所有其他验收；0.1.5三项均NotSigned，releaseAccepted=false，不重复索取证书。完整P3/P4/P5/P6仍未通过，目标active。下一步继续迁移实际故障恢复及维护操作整合，保留安装版入口完整串联、两类提交恢复按钮、签名/更新服务/干净目标，以及原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察。跨用户/路径/机器及图形迁移/统一程序数据事务尚未验收。WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[数据格式说明](DATA-FORMAT-MIGRATION-V1.md)；[102验收索引](../reports/p6-data-format-20261012.json)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old_latest = latest.read_text(encoding='utf8')
assert '卷002第010项（101）' in old_latest.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第011项 / 102' not in volume.read_text(encoding='utf8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md'
gates = root / 'docs/NEXT-STAGE-GATES.md'
readme = root / 'README.md'
text = readme.read_text(encoding='utf8')
start = text.index('最新进展（'); end = text.index('\n\n', start)
new_readme = text[:start] + '最新进展（2026-10-12 / 102）：0.1.5增加活动数据格式门槛及显式迁移；实机迁移、旧版拒绝、原路径备份恢复和维护窗口更新/回滚/提交通过。实际断电等故障矩阵、完整安装版入口及正式发布等仍待验收。用户暂无签名证书，继续其他验收。先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。' + text[end:]
# All evidence assertions and update preconditions above precede document writes.
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第011项（102），progress，活动数据格式门槛/显式迁移/旧版恢复通过，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方101为冻结历史。\n\n' + old_latest.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第011项 / 102 活动数据格式与显式迁移\n\n' + stamp + '\n\n' + body.replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)').replace('(../reports/', '(../../reports/'))
with matrix.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 102 活动数据格式与显式迁移（限定范围通过）\n\n' + stamp + '\n\n' + body)
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 102：活动数据容器1→2明确迁移、旧版拒绝与原路径恢复通过；0.1.5内部包和维护窗口更新/回滚/提交通过。完整阶段未通过。\n\n- 继续迁移实际故障恢复及维护操作整合；安装版入口串联、Reconcile/AbandonCommit窗口故障路径保留。\n- 签名证书暂无，继续其他验收；正式发布、更新服务和干净目标未通过。\n- 全部原始接入、真实管理/两桌面/编程效率、金额预算等范围不缩减；先复用预案和冻结报告。\n- 先读[最新交接](HANDOFF-LATEST.md)与[102证据](../reports/p6-data-format-20261012.json)，不重发WorkBuddy064。\n\n以下101及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme.write_text(new_readme, encoding='utf8')

names = ['Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json',
         'apps/desktop/src-tauri/tauri.conf.json', 'apps/desktop/src-tauri/src/maintenance.rs',
         'scripts/maintenance-window.ps1', 'scripts/manage-windows-update.ps1',
         'scripts/test-windows-package.ps1', 'scripts/check-package-candidate-20261012.ps1',
         'scripts/check-data-format-20261012.py', 'scripts/check-data-format-update-gate-20261012.ps1',
         'scripts/check-maintenance-workflow-20261012.ps1', 'scripts/finalize-data-format-20261012.py',
         'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md',
         'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
names += ['crates/core/src/' + name + '.rs' for name in ['lib', 'data_format', 'backup', 'upgrade', 'ipc', 'main', 'workbench_cli']]
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-data-format-*-20261012.log'))
for key, value in results.items():
    run = reports / Path(value['run']).name
    if key == 'nativeMigration':
        paths.update(p for p in run.rglob('*') if p.is_file())
    else:
        paths.update(run.glob('*.json')); paths.update(run.glob('*.txt'))
        paths.update(run.glob('transaction-*/*.json'))
        paths.update(run.glob('maintenance/*.json'))
        paths.update(run.glob('maintenance/operations/*'))
archive.mkdir()
records = []
for path in sorted(paths):
    rel = path.relative_to(root); target = archive / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, target)
    value = digest(path); assert digest(target) == value
    records.append({'source': rel.as_posix(), 'snapshot': target.relative_to(root).as_posix(), 'sha256': value})
report = {
    'batch': '102', 'at': stamp, 'status': 'explicit-data-container-migration-and-version-gates-verified',
    'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False,
    'fullP5Accepted': False, 'fullP6Accepted': False, 'realModelCalls': 0, 'userDesktopActions': 0,
    'results': results, 'workspaceTests': {'passed': totals[0], 'failed': totals[1], 'ignored': totals[2]},
    'formatCheck': 'cargo fmt --all -- --check passed', 'realMigrationCrashTested': False,
    'installedProductEntryThroughWholeWorkflowAccepted': False, 'scriptsEmbeddedInPackage': True,
    'publisherCertificateAvailable': False, 'userDirection': 'continue all other acceptance without waiting for certificate',
    'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path),
    'releaseRuntimeHashes': runtime_hashes, 'debugCoreSha256ObservedAtFreeze': digest(root / 'target/debug/cerulean-core.exe'),
    'debugDesktopAndRuntime': 'retained older 0.1.3 development build; not the 0.1.5 core',
    'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path),
    'previousArchiveRecordsVerified': len(previous['records']), 'previousPackageUnchanged': True,
    'remaining': ['real migration interruption/disk-full/power-loss recovery', 'migration UI and maintenance integration',
                  'installed product entry through complete maintenance chain', 'actual Reconcile/AbandonCommit UI fault flows',
                  'cross-user/path/machine migration', 'signing/update service/clean target',
                  'all original provider/management/desktop/coding/efficiency scope; 075/085 observations'],
    'records': records, 'archiveScope': 'selected sources and synthetic evidence, not complete rebuildable repository',
}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'workspaceTests': totals, 'goalStatus': 'active'}, ensure_ascii=False))
