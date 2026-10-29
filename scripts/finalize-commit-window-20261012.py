"""Freeze 105: real UI commit failures and recovery with unchanged product code."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root=Path(r'\\?\D:\test\CeruleanLonging'); reports=root/'reports'
index=reports/'p6-commit-window-20261012.json'; archive=reports/'p6-commit-window-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def result(name):
    raw=(reports/name).read_bytes()
    text=raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
    return json.loads([line for line in text.splitlines() if line.startswith('{')][-1])
previous_path=reports/'p6-data-window-20261012.json'; previous=load(previous_path)
for item in previous['records']:
    assert digest(root/item['snapshot'])==item['sha256'],item['snapshot']
    source=item['source']
    if source.startswith(('crates/','apps/')) or source in ['Cargo.toml','Cargo.lock','package.json','package-lock.json','scripts/maintenance-window.ps1','scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1']:
        assert digest(root/source)==item['sha256'],source
package_path=root/previous['packageManifest']; package=load(package_path)
assert digest(package_path)==previous['packageManifestSha256']
for item in package['artifacts']: assert digest(package_path.parent/item['path'])==item['sha256']
results={key:result(name) for key,name in {
    'held':'p6-commit-window-held1-20261012.log',
    'releasedFirstObservation':'p6-commit-window-released1-20261012.log',
    'releasedRecovery':'p6-commit-window-released2-20261012.log',
}.items()}
for key,item in results.items():
    assert load(reports/Path(item['run']).name/'result.json')==item
    assert item['cleanup']=='passed'
    assert item['status']==('failed' if key=='releasedFirstObservation' else 'passed')
    assert sum(x['action']=='Commit' for x in item['calls'])==1
for key in ['held','releasedRecovery']:
    item=results[key]
    assert item['actualWinFormsControls'] and item['displayedResultsVerified'] and item['reopenedBeforeRecovery'] and item['projectUnchanged']
    assert item['sourceVersion']=='0.1.5' and item['candidateVersion']=='0.1.6'
    assert item['stagingFixture'] and not item['productDesktopLauncherRetested']
held=results['held']; released=results['releasedRecovery']; late=results['releasedFirstObservation']
assert held['faultEvidence']['kind']=='real-hold-rename-sharing-violation' and not held['faultEvidence']['processKilled']
assert sum(x['action']=='AbandonCommit' for x in held['calls'])==1
assert sum(x['action']=='Rollback' for x in held['calls'])==1
assert released['preexistingArchiveEntries']==512
assert released['faultEvidence']['wasAlive'] and released['faultEvidence']['hasExited'] and released['faultEvidence']['exit']==-1
assert not released['faultEvidence']['error'] and sum(x['action']=='Reconcile' for x in released['calls'])==1
def output(item,action):
    call=next(x for x in item['calls'] if x['action']==action)
    receipt=load(Path(call['receipt']))
    return load(Path(receipt['stdout']))
assert output(held,'Review')['releaseObservation']['outcome']=='still-held'
assert output(released,'Review')['state']=='commit-intent'
assert output(released,'Review')['releaseObservation']['outcome']=='released'
assert output(released,'Reconcile')['state']=='committed'
assert output(late,'Review')['state']=='committed'
assert late['message']=='Original commit is not unresolved; do not create another fault'
late_commit=load(Path(next(x for x in late['calls'] if x['action']=='Commit')['receipt']))
assert late_commit['exit']==-1 and Path(late_commit['stdout']).stat().st_size==0
late_observation={'originalFaultAttemptStatus':'failed-to-capture-unresolved-window','actualTransactionState':'committed',
                  'commitStdoutBytes':0,'realProcessTerminationConfirmed':late['faultEvidence']['hasExited'],
                  'readOnlyReviewRecognizedCompletion':True,'commitAttempts':1,'reconcileAttempts':0,'automaticReplay':False}
stamp=datetime.now().astimezone().isoformat()
body='''105 补齐原冻结0.1.6维护窗口的两个提交恢复控件。产品代码、依赖、版本与安装包未变，沿用099事务及104窗口。只扩展实机验收驱动：操作失败也核对窗口实际显示和进程退出回执；点击后即记录待核对状态，捕获失败时不关闭仍有未知操作的原窗口。

第一条为真实Windows共享冲突：只读持有本次测试hold文件，实际窗口执行Commit已写commit-intent，但归档改名失败并退出1。释放测试句柄后关闭重开窗口，Review确认still-held，点击“放弃未完成提交”进入commit-abandoned，再点击“恢复原程序”回到0.1.5并保留hold。随后测试驱动明确解锁、核对原项目，再停止本次核心并卸载；结果和清理通过。未修改ACL、未伪造事务意图、未调用模型。

第二条为真实提交进程中断：窗口启动自己的提交子进程，驱动以父PID、暂存脚本及事务路径确认归属并捕获原进程句柄；文件监视器观察原hold归档发布后，仅终止该进程并等待确认退出。第一次实际终止晚于committed记录，退出-1、stdout为0；窗口重开正确读取已完成，驱动保留“未捕获目标未决窗口”的失败，不删除完成记录、不重发该事务，也未错误调用Reconcile。

随后使用全新隔离安装和512份既有归档夹具，让真实的基线扫描覆盖带历史数据的路径；没有给产品加停点或延迟。第二次实际退出-1，重开后原事务为commit-intent，唯一新增释放凭据支持released。通过“核对提交结果”进入committed，核对前后数据文件集合及摘要完全相同，没有再次释放；原项目保持、安装与登记清理通过。512份历史是明确的合成基线，不能说成512次真实业务提交，也不由这一例宣称性能/故障矩阵全部通过。

三次都只发送一次Commit，失败或中断后先读取同一事务。两条通过路径同时核对真实控件、进程回执与窗口显示，均用0.1.5隔离安装到0.1.6候选及明确暂存夹具；不等于完整安装版React入口。104冻结归档202项、所有相关产品源码和0.1.6包摘要保持，因此复用原构建、clippy、备份、候选/正常维护和迁移证据，不重复基础接入或重新打包。

本轮检查确认发行版从Windows系统目录读取默认数据，CERULEAN_TEST_DATA_DIR/缓存覆盖仅在debug_assertions启用；当前账户已有工作台数据。用户已回答暂无独立Windows测试环境、继续其他验收，所以安装版完整入口与干净机器仍待环境，不搬移现有账户数据或改写系统目录映射来代替隔离。暂无签名证书的先前决定继续有效，不重复询问。

完整P3/P4/P5/P6仍未通过，目标active。下一步继续无需独立Windows环境的磁盘满/存储失败与自动保留恢复机制，保留安装版完整入口、统一程序数据事务、跨用户路径机器、签名/更新服务/干净机器及所有原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察。真实模型/用户应用动作0；WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[维护窗口](MAINTENANCE-WINDOW-V1.md)；[105验收索引](../reports/p6-commit-window-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8');assert '卷002第013项（104）' in old.splitlines()[2]
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md';assert '第014项 / 105' not in volume.read_text(encoding='utf8')
readme=root/'README.md';text=readme.read_text(encoding='utf8');start=text.index('最新进展（');end=text.index('\n\n',start)
new_readme=text[:start]+'最新进展（2026-10-12 / 105）：实际窗口提交共享冲突后放弃/回滚、释放后进程中断并核对提交通过；0.1.6产品与包保持。用户暂无独立Windows环境，完整安装版入口和干净机器待验收，继续其他故障恢复工作。先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。'+text[end:]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第014项（105），progress，实际窗口提交故障与放弃/核对恢复通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方104为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
with volume.open('a',encoding='utf8') as stream:stream.write('\n\n## 第014项 / 105 提交故障与窗口恢复\n\n'+stamp+'\n\n'+body.replace('(MAINTENANCE-WINDOW-V1.md)','(../../docs/MAINTENANCE-WINDOW-V1.md)').replace('(../reports/','(../../reports/'))
with (root/'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a',encoding='utf8') as stream:stream.write('\n\n## 105 提交故障与窗口恢复（限定范围通过）\n\n'+stamp+'\n\n'+body)
gates=root/'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 105：实际窗口AbandonCommit/回滚与Reconcile故障路径通过，产品仍为冻结0.1.6。完整阶段未通过。\n\n- 继续不依赖隔离Windows的存储失败/磁盘满和自动保留恢复工作，既有完整窗口及迁移证据继承。\n- 用户确认暂无独立Windows测试环境；完整安装版入口和干净机器保留待验收，不重复询问，不操作当前账户既有数据作破坏性替代。\n- 用户暂无签名证书，继续其他验收；原全部接入/管理/编程/效率范围保持。\n- 先读[最新交接](HANDOFF-LATEST.md)与[105证据](../reports/p6-commit-window-20261012.json)，不重发WorkBuddy064。\n\n以下104及更早为历史。\n\n'+gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'),encoding='utf8')
readme.write_text(new_readme,encoding='utf8')
names=['scripts/check-maintenance-workflow-20261012.ps1','scripts/finalize-commit-window-20261012.py','scripts/maintenance-window.ps1','scripts/manage-windows-update.ps1','apps/desktop/src-tauri/src/main.rs','README.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/MAINTENANCE-WINDOW-V1.md','docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths={root/name for name in names}|{volume,package_path}
paths.update(reports.glob('p6-commit-window-*-20261012.log'))
for item in results.values():
    run=reports/Path(item['run']).name
    paths.update(run.glob('*.json'));paths.update(run.glob('transaction-*/*.json'))
    paths.update(run.glob('transaction-*/data-backup/manifest.json'))
    paths.update(run.glob('maintenance/*.json'));paths.update(run.glob('maintenance/operations/*'))
    paths.update(p for p in (run/'data').rglob('*') if p.is_file())
archive.mkdir();records=[]
for path in sorted(paths):
    rel=path.relative_to(root);target=archive/rel;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(path,target)
    value=digest(path);assert digest(target)==value
    records.append({'source':rel.as_posix(),'snapshot':target.relative_to(root).as_posix(),'sha256':value})
report={'batch':'105','at':stamp,'status':'actual-ui-commit-failure-abandon-and-reconcile-verified','goalStatus':'active','fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'results':results,'lateCompletionObservation':late_observation,'realModelCalls':0,'userDesktopActions':0,'productSourceUnchanged':True,'packageUnchanged':True,'packageManifest':package_path.relative_to(root).as_posix(),'packageManifestSha256':digest(package_path),'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'previousArchiveRecordsVerified':len(previous['records']),'publisherCertificateAvailable':False,'isolatedWindowsEnvironmentAvailable':False,'userDirection':'continue other acceptance without an isolated Windows environment or signing certificate','installedReactEntryThroughWholeFlowAccepted':False,'remaining':['storage failure/disk-full/power-loss matrix','automatic preservation/restore','unified program-data transaction','cross-user/path/machine migration','installed React entry and clean Windows target need isolated environment','signing/update service','all original provider/management/desktop/coding/efficiency scope; 075/085 observations'],'records':records,'archiveScope':'selected sources, synthetic histories, real UI and process evidence; not a full rebuildable repository'}
with index.open('x',encoding='utf8') as stream:json.dump(report,stream,ensure_ascii=False,indent=2);stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'goalStatus':'active'},ensure_ascii=False))
