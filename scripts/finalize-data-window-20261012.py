"""Freeze 104: independent data-maintenance window and package 0.1.6."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root=Path(r'\\?\D:\test\CeruleanLonging'); reports=root/'reports'
index=reports/'p6-data-window-20261012.json'; archive=reports/'p6-data-window-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(name):
    raw=(reports/name).read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
def result(name): return json.loads([line for line in log(name).splitlines() if line.startswith('{')][-1])
previous_path=reports/'p6-migration-interrupt-20261012.json'; previous=load(previous_path)
for entry in previous['records']:
    assert digest(root/entry['snapshot'])==entry['sha256'],entry['snapshot']
    if entry['source']=='crates/core/src/data_format.rs': assert digest(root/entry['source'])==entry['sha256']
old_package=root/previous['packageManifest']; assert digest(old_package)==previous['packageManifestSha256']
for item in load(old_package)['artifacts']: assert digest(old_package.parent/item['path'])==item['sha256']
results={key:result(name) for key,name in {
    'preview':'p6-data-window-preview2-20261012.log',
    'dataWindow':'p6-data-window-native1-20261012.log',
    'programRegression':'p6-data-window-program-20261012.log',
    'candidate':'p6-data-window-candidate-20261012.log',
}.items()}
for key,item in results.items():
    assert item['status']=='passed',key
    if key!='candidate': assert load(reports/Path(item['run']).name/'result.json')==item
for item in results['preview']['results']:
    expected=5 if item['mode']=='data' else 7
    assert item['description']['confirmationGateCases']==expected
    assert item['description']['automaticActions']==0
native=results['dataWindow']; program=results['programRegression']
for item in [native,program]:
    assert item['cleanup']=='passed' and item['displayedResultsVerified'] and item['currentTools']
    assert item['projectUnchanged'] and item['stagingFixture'] and not item['productDesktopLauncherRetested']
    assert item['realModelCalls']==0 and item['userDesktopActions']==0
for key in ['parentButtonLaunchedDataWindow','formatMigration','reopenedResultVerified','originalLocationRestoreExact','preservedDirectoryByTestDriver']:
    assert native[key],key
assert native['sourceVersion']=='0.1.6' and native['explicitReleaseCount']==2
assert sum(x['action']=='formatMigrate' for x in native['calls'])==1
assert sum(x['action']=='dataRestore' for x in native['calls'])==1
assert sum(x['action']=='dataRelease' for x in native['calls'])==2
assert program['sourceVersion']=='0.1.5' and program['candidateVersion']=='0.1.6'
assert program['commit'] and program['windowReopenedBeforeRollback']
assert results['candidate']['passed']==12
assert 'Unexpected control or consent count' in log('p6-data-window-preview1-20261012.log')
assert '8 passed; 0 failed' in log('p6-data-window-backup-20261012.log')
assert 'Finished' in log('p6-data-window-clippy-20261012.log')
assert 'Finished 1 bundle' in log('p6-data-window-package-build-20261012.log')
assert not log('p6-data-window-fmt-20261012.log').strip()
package_path=reports/'p6-windows-package-016-20261012/manifest.json'; package=load(package_path)
assert package['version']=='0.1.6' and not package['releaseAccepted']
for item in package['artifacts']:
    assert digest(package_path.parent/item['path'])==item['sha256']
    assert item['authenticodeStatus']=='NotSigned'
image=(package_path.parent/'CeruleanLonging.exe').read_bytes()
for name in ['maintenance-window.ps1','manage-windows-update.ps1','test-windows-package.ps1']:
    assert (root/'scripts'/name).read_bytes() in image,name
runtime_names=['target/release/cerulean-core.exe','target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime_hashes={name:digest(root/name) for name in runtime_names}; assert set(runtime_hashes.values())=={package['coreSha256']}
stamp=datetime.now().astimezone().isoformat()
body='''104 将已验收的CLI数据迁移/恢复接入独立维护窗口。原程序维护窗口增加“打开数据迁移与恢复”，使用同一已校验副本、SID、工具摘要与原暂存核心，启动回执保存PID及会话摘要，打开不会操作数据，回执不等于窗口已加载。新窗口独立持有工具句柄，父窗口可正常关闭。Tauri页面更新使用说明；原Rust暂存/启动路径保持。

数据窗口支持核心审阅/锁定、锁定备份、核对备份、迁移审阅/确认、读取格式、原位置恢复与明确解锁。更改备份目录/摘要会清除迁移审阅，迁移发送前消耗审阅；解除前需成功读取当前格式及有效hold，底层CLI再次核验。每次变更单独确认，子进程进行中禁用控件和窗口关闭，原请求/输出/退出回执保留，沿用101输出收尾，不超时重发、不自动开核心。

实际0.1.6隔离安装中，以原维护窗口按钮打开数据窗口，执行备份、迁移、关闭重开读回格式2、明确解锁，原项目保持。随后测试驱动核对路径归属、保留整个迁移后目录，实际窗口核对备份并恢复原位置；全部文件与备份摘要一致，读回格式1及原hold，再明确解锁，原项目仍保持。显示结果与磁盘回执逐项一致，两次解除均明确执行，安装及登记清理通过。数据目录移走由测试驱动完成，不代表产品已有自动保留/搬移恢复；产品恢复仍拒绝覆盖现存目录。

共享控制器的原程序更新流程回归通过：0.1.5→0.1.6更新、关闭重开后回滚、再更新并提交，原项目及清理通过。两项均为明确组装的维护暂存夹具，复用100已验证启动机制；未声称从已安装React入口一路执行完整操作链。程序更新与数据迁移仍是两个明确操作，先完成程序更新并从新版工作台重开维护副本，不在未提交程序事务中插入迁移。不同会话不是一个统一程序/数据事务。

两种窗口预览已人工查看，实际13/10个按钮及7/5个确认门槛通过，没有触发外部操作。首轮发现PowerShell单元素嵌套数组展开导致新增按钮被拆成字符，改为明确一元数组后通过；原失败日志、错误控件清单和预览保留。八项备份、格式检查、严格workspace全targets clippy、完整前端/release/NSIS构建与12项候选检查通过；依赖版本保持，原迁移算法及103故障证据继承，不重做基础接入。

版本统一推进0.1.6并扩展备份writer列表；三脚本原字节嵌入最终GUI，release三处runtime一致。包仍NotSigned、releaseAccepted=false，用户暂无证书并要求继续其他验收。103冻结归档270项及0.1.5包保持。debug核心/桌面/runtime不是同一版本组合，实机使用冻结release包，后续开发需正常重建。完整P3/P4/P5/P6仍未通过，目标active；下一步继续完整安装版入口串联、两类提交恢复按钮及磁盘满/剩余故障矩阵，保留统一程序数据事务/自动保留恢复、跨用户路径机器、签名更新服务/干净机器及全部原首批接入、真实OpenAI管理/编程/含失败效率、金额预算/按执行者监督和075/085观察。真实模型/用户应用动作0，WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[维护窗口](MAINTENANCE-WINDOW-V1.md)；[104验收索引](../reports/p6-data-window-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md'; old=latest.read_text(encoding='utf8'); assert '卷002第012项（103）' in old.splitlines()[2]
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'; assert '第013项 / 104' not in volume.read_text(encoding='utf8')
readme=root/'README.md'; text=readme.read_text(encoding='utf8'); start=text.index('最新进展（');end=text.index('\n\n',start)
new_readme=text[:start]+'最新进展（2026-10-12 / 104）：0.1.6增加独立数据迁移/恢复窗口；实际备份、迁移、重开核对、原位置恢复及解锁通过，原程序更新/回滚/提交回归通过。生产恢复前保留旧目录仍需另行操作，完整安装版入口、故障矩阵和正式发布未完成。先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。'+text[end:]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第013项（104），progress，数据维护窗口与迁移/恢复操作链通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方103为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
with volume.open('a',encoding='utf8') as stream: stream.write('\n\n## 第013项 / 104 数据维护窗口与恢复操作链\n\n'+stamp+'\n\n'+body.replace('(MAINTENANCE-WINDOW-V1.md)','(../../docs/MAINTENANCE-WINDOW-V1.md)').replace('(../reports/','(../../reports/'))
with (root/'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a',encoding='utf8') as stream: stream.write('\n\n## 104 数据维护窗口（限定范围通过）\n\n'+stamp+'\n\n'+body)
gates=root/'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 104：0.1.6数据维护窗口的备份/迁移/重开/恢复/解锁通过，原程序更新路径回归通过。完整阶段未通过。\n\n- 下一步推进安装版完整入口串联、Reconcile/AbandonCommit真实UI故障路径、磁盘满/剩余故障；既有迁移正常/进程终止证据继续继承。\n- 目录保留目前仍由用户另行操作；程序与数据是两个事务，不宣称统一自动迁移回滚。\n- 用户暂无证书，继续其他验收；所有原接入、真实管理/两桌面/编程效率等范围保持。\n- 先读[最新交接](HANDOFF-LATEST.md)与[104证据](../reports/p6-data-window-20261012.json)，不重发WorkBuddy064。\n\n以下103及更早为历史。\n\n'+gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'),encoding='utf8')
readme.write_text(new_readme,encoding='utf8')
names=['Cargo.toml','Cargo.lock','package.json','package-lock.json','crates/core/src/lib.rs','crates/core/src/data_format.rs','apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/src/maintenance.rs','apps/desktop/src/Maintenance.tsx','scripts/maintenance-window.ps1','scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1','scripts/check-maintenance-workflow-20261012.ps1','scripts/check-data-window-preview-20261012.ps1','scripts/finalize-data-window-20261012.py','README.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/MAINTENANCE-WINDOW-V1.md','docs/DATA-FORMAT-MIGRATION-V1.md','docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths={root/name for name in names}|{volume,package_path}
paths.update(reports.glob('p6-data-window-*-20261012.log'))
for key,item in results.items():
    run=reports/Path(item['run']).name
    paths.update(run.glob('*.json'))
    paths.update(run.glob('transaction-*/*.json'))
    paths.update(run.glob('maintenance/*.json')); paths.update(run.glob('maintenance/operations/*'))
    if key=='dataWindow':
        for folder in ['data','data-backup','preserved-migrated-data']:
            paths.update(p for p in (run/folder).rglob('*') if p.is_file())
    if key=='preview':
        paths.update(p for p in run.rglob('*') if p.is_file() and p.suffix!='.exe')
failed_preview=reports/'data-window-preview-3419eece87e942afb55c906d9dcb3201'
paths.update(p for p in failed_preview.rglob('*') if p.is_file() and p.suffix!='.exe')
archive.mkdir();records=[]
for path in sorted(paths):
    rel=path.relative_to(root);target=archive/rel;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(path,target)
    value=digest(path);assert digest(target)==value;records.append({'source':rel.as_posix(),'snapshot':target.relative_to(root).as_posix(),'sha256':value})
report={'batch':'104','at':stamp,'status':'data-maintenance-window-migration-and-restore-verified','goalStatus':'active','fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'results':results,'previewsVisuallyInspected':True,'installedReactEntryThroughWholeFlowAccepted':False,'automaticDataDirectoryPreservationAccepted':False,'unifiedProgramAndDataTransactionAccepted':False,'realModelCalls':0,'userDesktopActions':0,'publisherCertificateAvailable':False,'packageManifest':package_path.relative_to(root).as_posix(),'packageManifestSha256':digest(package_path),'scriptsEmbeddedInPackage':True,'releaseRuntimeHashes':runtime_hashes,'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'previousArchiveRecordsVerified':len(previous['records']),'previousPackageUnchanged':True,'remaining':['installed React entry through complete maintenance flows','Reconcile/AbandonCommit UI fault flows','disk-full/power-loss/remaining storage faults','unified program-data transaction and automatic preservation','cross-user/path/machine migration','signing/update service/clean target','all original provider/management/desktop/coding/efficiency scope; 075/085 observations'],'records':records,'archiveScope':'selected sources and synthetic evidence; not full rebuildable repository'}
with index.open('x',encoding='utf8') as stream: json.dump(report,stream,ensure_ascii=False,indent=2);stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'goalStatus':'active'},ensure_ascii=False))
