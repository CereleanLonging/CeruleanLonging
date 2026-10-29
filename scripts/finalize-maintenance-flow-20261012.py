"""Freeze native maintenance controls and output-drain repair evidence (101)."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root=Path(r'\\?\D:\test\CeruleanLonging'); reports=root/'reports'
index=reports/'p6-maintenance-flow-20261012.json'; archive=reports/'p6-maintenance-flow-sources-20261012'
assert not index.exists() and not archive.exists(), 'Do not overwrite frozen evidence'
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(name):
    raw=(reports/name).read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
def result(name): return json.loads([line for line in log(name).splitlines() if line.startswith('{')][-1])
previous_path=reports/'p6-maintenance-window-20261012.json'; previous=load(previous_path)
for entry in previous['records']:
    assert digest(root/entry['snapshot'])==entry['sha256'],entry['snapshot']
    if entry['source'] in ('scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1','apps/desktop/src-tauri/src/maintenance.rs','apps/desktop/src-tauri/src/main.rs','apps/desktop/src/Maintenance.tsx','apps/desktop/src/App.tsx'):
        assert digest(root/entry['source'])==entry['sha256'],entry['source']
old_package=root/previous['packageManifest']; assert digest(old_package)==previous['packageManifestSha256']
for entry in load(old_package)['artifacts']: assert digest(old_package.parent/entry['path'])==entry['sha256']
results={str(i):result(f'p6-maintenance-workflow{i}-20261012.log') for i in range(1,9)}
for number,item in results.items():
    run=reports/Path(item['run']).name
    assert load(run/'result.json')==item
    if number in ('1','6'): assert load(run/'cleanup.json')['status']=='passed'
    else: assert item['cleanup']=='passed'
    assert item['status']==('passed' if int(number)>=7 else 'failed')
    if int(number)>=7:
        assert item['displayedResultsVerified'] and item['windowReopenedBeforeRollback'] and item['commit'] and item['projectUnchanged']
        assert item['stagingFixture'] and not item['productDesktopLauncherRetested']
        assert sum(c['action']=='Apply' for c in item['calls'])==2
        assert sum(c['action']=='Rollback' for c in item['calls'])==1
        assert sum(c['action']=='Commit' for c in item['calls'])==1
assert results['8']['sourceVersion']=='0.1.3' and results['8']['candidateVersion']=='0.1.4'
assert '8 passed; 0 failed' in log('p6-maintenance-flow-backup-20261012.log')
assert 'Finished' in log('p6-maintenance-flow-clippy-20261012.log')
assert 'Finished 1 bundle' in log('p6-maintenance-flow-build-20261012.log')
package_path=reports/'p6-windows-package-014-20261012/manifest.json'; package=load(package_path)
assert package['version']=='0.1.4' and not package['releaseAccepted']
for entry in package['artifacts']: assert digest(package_path.parent/entry['path'])==entry['sha256']
# include_str! embeds the exact controller in the shipped GUI image.
controller=root/'scripts/maintenance-window.ps1'
assert controller.read_bytes() in (package_path.parent/'CeruleanLonging.exe').read_bytes()
runtime_names=['target/release/cerulean-core.exe','target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime_hashes={name:digest(root/name) for name in runtime_names}; assert len(set(runtime_hashes.values()))==1
stamp=datetime.now().astimezone().isoformat()
body='''101 补齐实际WinForms控件到更新事务的编程外维护路径：核心审阅/锁定、读取包、备份准备、更新、关闭并重开窗口读取原事务、回滚，再次准备/更新/提交。最终使用真实0.1.3安装和0.1.4候选，程序与登记版本核对、原项目状态保持、测试安装和登记清理通过。每项成功命令同时检查磁盘完成回执和窗口实际显示，不只读取底层脚本结果。未发送真实模型或用户桌面消息。

专项采用隔离安装/数据与明确组装的维护副本，沿用100已验收的Rust桌面启动/暂存机制；此次没有从已安装产品的React按钮开启整条链，不能合称“整套已安装桌面发布流程通过”。真实控件通过限定PID/父窗口的原生消息触发，原窗口事件处理器、进程启动、输出定时器及按钮状态均实际执行；没有直接调用窗口StartAction替代点击。Reconcile/AbandonCommit按钮本批未制造故障执行，底层099恢复证据继续继承。

发现并修复产品输出收尾竞态：HasExited成立后，Start-Process异步重定向仍可能占用stdout。旧窗口在Apply已installed-held且工作进程退出后读取文件失败，缺finished记录但按钮释放；原现场保留、未重发Apply。新窗口在.NET后台任务WaitForExit等待输出处理器完成，UI定时器只观察任务，避免阻塞界面；完成后先落进程退出回执，再读日志和显示。原事务/候选审查脚本摘要不变。

修复后原0.1.2→0.1.3控制器专项通过，再以最终0.1.3→0.1.4包通过同一完整窗口路径。两项均更新两次、回滚一次、提交一次、窗口重开后恢复一次。保持首次到第六次失败及清理：隐藏第一窗的测试启动方式；UIA把旧WinForms控件统一暴露Pane；PS5测试读取器默认编码；自绘复选框BM_GETCHECK不能代表托管状态；最终上述真实输出竞态。控件等待假设被实测否定后按真实原生类/句柄修正，没有继续盲目加长等待。第1/6次关联进程先核对后正常关闭，单独cleanup记录保留；其余均原脚本清理通过。

版本推进到0.1.4并扩展schema1备份writer白名单，活动数据格式未改变。格式、严格workspace全targets clippy、八项备份专项、完整前端/release/NSIS构建通过；最终GUI二进制包含修复脚本的完整字节，release三处runtime一致。0.1.4三项包仍NotSigned，releaseAccepted=false。此次实际升级始于0.1.3 NSIS安装，未对0.1.4 NSIS再次做独立干净安装；100原安装/占用拒绝路径未改、继承已有结论。debug仍为100的0.1.3开发构建，不能当0.1.4运行。

用户本轮确认尚无Windows代码签名证书，要求继续其他验收。签名保留未完成，不再等待或重复询问，不把内部通道当正式发布。完整P3/P4/P5/P6仍未通过，目标active；接下来优先活动数据格式门槛/迁移与其余实际故障恢复，并保留完整安装版入口串联、两类提交恢复按钮、签名/更新服务/干净机器及全部原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察。WorkBuddy064不重发，不关机、不关闭用户程序、无提交推送。

[维护窗口](MAINTENANCE-WINDOW-V1.md)；[101验收索引](../reports/p6-maintenance-flow-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md'; old=latest.read_text(encoding='utf8'); assert '卷002第009项（100）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第010项（101），progress，实际维护窗口更新/重开回滚/提交与输出收尾修复通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方100为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'; assert '第010项 / 101' not in volume.read_text(encoding='utf8')
with volume.open('a',encoding='utf8') as stream: stream.write('\n\n## 第010项 / 101 维护窗口完整操作与输出收尾\n\n'+stamp+'\n\n'+body.replace('(MAINTENANCE-WINDOW-V1.md)','(../../docs/MAINTENANCE-WINDOW-V1.md)').replace('(../reports/','(../../reports/'))
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
with matrix.open('a',encoding='utf8') as stream: stream.write('\n\n## 101 实际维护窗口操作（限定范围通过）\n\n'+stamp+'\n\n'+body)
window_doc=root/'docs/MAINTENANCE-WINDOW-V1.md'; old=window_doc.read_text(encoding='utf8')
window_doc.write_text('# 桌面维护入口与独立维护窗口\n\n101 更新：实际窗口升级、重开读取原事务后回滚、再次升级并提交通过；0.1.4修复子进程退出早于输出重定向收尾的竞态。每项动作同时核对回执与实际窗口显示，原项目保持。使用隔离暂存夹具，安装版React入口的完整串联及两类故障提交恢复按钮仍待验收。用户确认无签名证书，继续其他工作。见[101证据](../reports/p6-maintenance-flow-20261012.json)。\n\n输出等待在.NET后台任务执行，定时器不阻塞UI；进程退出回执先于日志解析落盘。若等待未完成，不以超时重发，先核对原进程和事务。\n\n下方100说明保留；其中“尚未完成全部按钮操作链”由上述限定范围更新。\n\n'+old.removeprefix('# 桌面维护入口与独立维护窗口\n\n'),encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md'; old=gates.read_text(encoding='utf8')
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 101：维护窗口正常更新/重开回滚/提交通过，修复输出收尾竞态并分发0.1.4内部包。完整阶段未通过。\n\n- 优先活动数据格式门槛、迁移和剩余真实故障恢复；完整安装版React到维护流程串联、Reconcile/AbandonCommit控件实际故障路径仍保留。\n- 用户已确认没有签名证书，继续其他验收，不再等待答复；正式签名仍未通过。\n- 原全部首批接入、真实主管、两桌面、编程与效率、金额预算等范围不缩减。\n- 先读[最新交接](HANDOFF-LATEST.md)与[101证据](../reports/p6-maintenance-flow-20261012.json)，不重发WorkBuddy064。\n\n以下100及更早为历史。\n\n'+old.removeprefix('# 后续阶段的验收执行顺序\n\n'),encoding='utf8')
readme=root/'README.md'; text=readme.read_text(encoding='utf8'); start=text.index('最新进展（'); end=text.index('\n\n',start)
text=text[:start]+'最新进展（2026-10-12 / 101）：0.1.4修复维护窗口读取输出的竞态；实际窗口更新、重开回滚和提交通过，原项目保持。完整安装版入口串联、数据迁移与正式发布等仍待验收。用户暂无签名证书，继续其他验收。先读[最新交接](docs/HANDOFF-LATEST.md)。完整P0–P6尚未通过。'+text[end:]; readme.write_text(text,encoding='utf8')
names=['Cargo.toml','Cargo.lock','package.json','package-lock.json','crates/core/src/lib.rs','apps/desktop/src-tauri/tauri.conf.json','scripts/maintenance-window.ps1','scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1','scripts/check-maintenance-workflow-20261012.ps1','scripts/finalize-maintenance-flow-20261012.py','README.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/MAINTENANCE-WINDOW-V1.md','docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths={root/name for name in names}|{volume,package_path}; paths.update(reports.glob('p6-maintenance-workflow*-20261012.log')); paths.update(reports.glob('p6-maintenance-flow-*-20261012.log'))
for item in results.values():
    run=reports/Path(item['run']).name; paths.update(run.glob('*.json'));paths.update(run.glob('transaction-*/*.json')); paths.update(run.glob('maintenance/*.json')); paths.update(run.glob('maintenance/operations/*'))
archive.mkdir(); records=[]
for path in sorted(paths):
    rel=path.relative_to(root); target=archive/rel; target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(path,target)
    value=digest(path); assert digest(target)==value; records.append({'source':rel.as_posix(),'snapshot':target.relative_to(root).as_posix(),'sha256':value})
report={'batch':'101','at':stamp,'status':'native-maintenance-workflow-and-output-drain-repair-verified','goalStatus':'active','fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'realModelCalls':0,'userDesktopActions':0,'results':results,'installedProductEntryThroughWholeWorkflowAccepted':False,'controllerEmbeddedInPackage':True,'publisherCertificateAvailable':False,'userDirection':'continue all other acceptance without waiting for certificate','packageManifest':package_path.relative_to(root).as_posix(),'packageManifestSha256':digest(package_path),'releaseRuntimeHashes':runtime_hashes,'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'previousArchiveRecordsVerified':len(previous['records']),'previousPackageUnchanged':True,'remaining':['active data format gates/migration','remaining real fault recovery','installed product entry through complete maintenance chain','actual Reconcile/AbandonCommit UI fault flows','signing/update service/clean target','all original provider/management/desktop/coding/efficiency scope'],'records':records,'archiveScope':'selected source and synthetic maintenance evidence; not complete rebuildable repository'}
with index.open('x',encoding='utf8') as stream: json.dump(report,stream,ensure_ascii=False,indent=2);stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'goalStatus':'active'},ensure_ascii=False))
