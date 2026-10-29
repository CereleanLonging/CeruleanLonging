"""Freeze commit reconciliation evidence; previous packages and reports remain immutable."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json,shutil
root=Path(r'\\?\D:\test\CeruleanLonging');reports=root/'reports'
index=reports/'p6-commit-recovery-20261012.json';archive=reports/'p6-commit-recovery-sources-20261012'
assert not index.exists() and not archive.exists(),'Preserve frozen evidence'
def digest(p):return sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def native(stage):
    raw=(reports/f'p6-commit-{stage}-20261012.log').read_bytes()
    text=raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
    return json.loads([line for line in text.splitlines() if line.startswith('{')][-1])
previous_path=reports/'p6-transaction-20261012.json';previous=load(previous_path)
for item in previous['records']:
    assert digest(root/item['snapshot'])==item['sha256'],item['snapshot']
    if item['source'].startswith('crates/') or item['source'] in {'Cargo.toml','Cargo.lock','package.json','package-lock.json','apps/desktop/src-tauri/tauri.conf.json','scripts/test-windows-package.ps1'}:
        assert digest(root/item['source'])==item['sha256'],item['source']
package=root/previous['packageManifest'];assert digest(package)==previous['packageManifestSha256']
for item in load(package)['artifacts']:assert digest(package.parent/item['path'])==item['sha256']
for name,value in previous['releaseRuntimeHashes'].items():assert digest(root/name)==value
results={name:native(name) for name in ['held1','released1','released2','legacy1','crash1','crash-released1','transaction-regression']}
for name,result in results.items():
    assert load(reports/Path(result['run']).name/'result.json')==result
    assert result['cleanup']=='passed'
    assert result['status']==('failed' if name=='released1' else 'passed')
    if name!='released1':assert result.get('realModelCalls',result.get('modelCalls'))==0 and result['userDesktopActions']==0
for name in ['crash1','crash-released1']:
    c=results[name]['crash'];assert c['processHandleCaptured'] and c['hasExited'] and c['waitReturned']
    assert c['observedState']=='commit-intent'
assert results['crash1']['crash']['releaseObservation']['outcome']=='still-held'
assert results['crash-released1']['crash']['releaseObservation']['outcome']=='released'
assert results['released2']['recovery']=='original-release-reconciled-without-data-write'
assert results['legacy1']['recovery']=='original-release-reconciled-without-data-write'
assert results['transaction-regression']['actualProgramReplacement'] and results['transaction-regression']['actualProgramRollback'] and results['transaction-regression']['projectUnchanged']
stamp=datetime.now().astimezone().isoformat()
body='''099 补齐升级提交的原凭据对账与明确放弃。Review对未决提交在原数据写锁可取得时区分released/still-held/conflict/unproven，不可读或核心活动仅报告unavailable。只有原hold缺失、备份基线之后恰有一个命名和全部字节摘要匹配的新增归档才证明释放；旧归档、重复、改动或原hold同时存在不算成功。

Reconcile重新核对源/候选包、备份、当前程序和登记，持有事务与数据锁后仅追加committed，不修改核心数据、不调用release、不重开项目。已释放后的合法数据使用不要求回退到提交前快照。AbandonCommit仅对标明in-process-v1的未决意图开放：原hold保持、没有新增释放凭据、原数据匹配时记录commit-abandoned，之后只能明确Rollback，不能重发Commit。允许中断释放留下的固定空upgrade-holds目录差异，其余新增文件仍拒绝。

原098独立release子进程在父维护进程退出后可能继续，所以仅看到hold仍在不能证明未来不会释放。新Commit保持原writer.lock，在维护进程内把已经核验的原hold以原history格式、禁止覆盖改名归档，再记录committed。父进程退出后不存在本动作的独立释放子进程；CLI原release命令没有修改。旧无releaseMode记录若有唯一新凭据仍能收口，但仅原hold保持时不允许放弃，需人工审阅旧子进程生命周期，未伪称该旧状态自动恢复。

五类专项在Windows PowerShell5通过：未释放意图明确放弃/回滚；真实核心释放后仅对账且数据文件集合/摘要不变；篡改/重复/冲突凭据及活动核心拒绝；旧格式成功凭据兼容且排除已有旧归档；两个真实提交进程终止窗口。释放前进程14028经原句柄确认退出，观察commit-intent+still-held后放弃并回滚；释放后进程25100确认退出，观察commit-intent+原hold已归档、committed未写入后Reconcile收口，未再次释放。两次退出等待均完成、HasExited=true、返回-1，均仅本次测试进程。

原098完整更新/回滚/正常提交检查作为改动相关回归通过，含另一次apply意图后本次维护进程终止；项目状态保持，原安装/登记均清理。原Rust、依赖、前端、候选检查器与0.1.2包未改，继承098相应编译/专项，不重建或重复旧接入。真实模型/用户桌面动作0，WorkBuddy064不重发。released首轮在正则自动变量Matches覆盖同名数组处失败，改为明确归档数组名；失败报告及cleanup=passed保留，后续正例/旧格式/实际终止与正常提交均通过。

完整P3/P4/P5/P6仍未通过，目标active。下一步将维护工具作为明确版本化组件分发并接入升级界面，继续活动数据格式门槛/迁移、剩余真实进程/磁盘满/断电窗口、签名与更新服务、干净机器。旧legacy未释放意图限制保留。全部原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察不缩减。证书身份问题仍待答复，不阻止其他工作；不关机、不关闭用户程序，无提交推送。

[更新事务说明](WINDOWS-UPDATE-TRANSACTION-V1.md)；[099验收索引](../reports/p6-commit-recovery-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8');assert '卷002第007项（098）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第008项（099），progress，提交结果对账与释放前/后真实进程终止恢复通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方098为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md';assert '第008项 / 099' not in volume.read_text(encoding='utf8')
handoff_body=body.replace('(WINDOWS-UPDATE-TRANSACTION-V1.md)','(../../docs/WINDOWS-UPDATE-TRANSACTION-V1.md)').replace('(../reports/','(../../reports/')
with volume.open('a',encoding='utf8') as f:f.write('\n\n## 第008项 / 099 提交结果对账与进程终止恢复\n\n'+stamp+'\n\n'+handoff_body)
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
matrix.write_text(matrix.read_text(encoding='utf8').replace('本页098小节','本页099小节')+'\n\n## 099 提交结果对账（限定范围通过）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';old_gates=gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 099：升级提交前/后两次真实进程终止、原凭据对账和明确放弃通过。原更新/回滚/正常提交回归通过，0.1.2包保持。完整P3/P4/P5/P6未通过。见[更新事务](WINDOWS-UPDATE-TRANSACTION-V1.md)、[最新交接](HANDOFF-LATEST.md)。

- 下一步：把独立维护工具作为版本化组件分发并接升级UI；现尚需工程目录执行脚本，不是完整桌面升级入口。
- 继续活动数据格式门槛/迁移、其他真实进程/断电/磁盘满窗口、并发启动、签名/更新服务与干净目标；备份兼容和固定双程序更新不代证这些门槛。
- 旧098无releaseMode且仍held的意图不能自动放弃，旧释放子进程生命周期无证明，保留人工审阅限制；已有唯一新归档的旧意图可以对账。新意图释放在同进程原写锁下完成。
- 全部原首批接入、真实OpenAI管理/编程/含失败效率、两桌面/共享故障负载、金额预算/按执行者监督及075/085观察保持。先查原型，不重发WorkBuddy064。本日卷002第008项，旧冻结证据保持。
- 正式签名证书身份仍待用户答复；debug主程序仍为旧开发构建，不能与release混称。

'''+old_gates[old_gates.index('以下为较早阶段的历史记录。'):],encoding='utf8')
paths={root/r['source'] for r in previous['records'] if r['source'].startswith('crates/')}
for name in ['scripts/manage-windows-update.ps1','scripts/test-windows-package.ps1','scripts/check-update-transaction-20261012.ps1','scripts/check-update-commit-20261012.ps1','scripts/finalize-commit-recovery-20261012.py','docs/WINDOWS-UPDATE-TRANSACTION-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','README.md','Cargo.toml','Cargo.lock']:
    paths.add(root/name)
paths.add(volume);paths.update(reports.glob('p6-commit-*-20261012.log'))
for result in results.values():
    run=reports/Path(result['run']).name;paths.add(run/'result.json')
    paths.update(run.glob('transaction-*/*.json'));paths.update(run.glob('commit-child.*'));paths.update(run.glob('interrupted-updater.*'))
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=digest(p);assert digest(dest)==value;records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'099','at':stamp,'status':'commit-reconciliation-and-before-after-release-process-exit-verified','goalStatus':'active',
    'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'realModelCalls':0,'userDesktopActions':0,
    'commitCrashWindows':2,'results':results,'packageUnchanged':True,'packageManifest':previous['packageManifest'],'packageManifestSha256':previous['packageManifestSha256'],
    'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),
    'remaining':['versioned tool distribution and upgrade UI','active data format gates/migration','remaining crash/power-loss/disk-full matrix','legacy still-held commit manual review','signing/update service/clean target','full original real-provider/desktop/coding/efficiency scope'],
    'archiveScope':'selected source and synthetic transaction evidence, not complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'goalStatus':'active'},ensure_ascii=False))
