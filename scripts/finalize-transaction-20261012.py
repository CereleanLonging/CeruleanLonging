"""Freeze batch 098 evidence without modifying any previous frozen archive."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json,shutil
root=Path(r'\\?\D:\test\CeruleanLonging');reports=root/'reports'
index=reports/'p6-transaction-20261012.json';archive=reports/'p6-transaction-sources-20261012'
assert not index.exists() and not archive.exists(),'Preserve existing evidence'
def digest(p):return sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def log(stage):
    raw=(reports/f'p6-transaction-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
def native(stage):return json.loads([x for x in log(stage).splitlines() if x.startswith('{')][-1])
previous_path=reports/'p6-candidate-20261012.json';previous=load(previous_path)
for entry in previous['records']:assert digest(root/entry['snapshot'])==entry['sha256'],entry['snapshot']
for entry in previous['records']:
    if entry['source'].startswith('crates/') and '/src/' in entry['source'] and entry['source'] not in {'crates/core/src/main.rs','crates/core/src/lib.rs'}:
        assert digest(root/entry['source'])==entry['sha256'],entry['source']
old_package=root/previous['packageManifest'];assert digest(old_package)==previous['packageManifestSha256']
for item in load(old_package)['artifacts']:assert digest(old_package.parent/item['path'])==item['sha256']
package=reports/'p6-windows-package-012-20261012/manifest.json';manifest=load(package)
assert manifest['version']=='0.1.2' and not manifest['releaseAccepted']
assert manifest['coreBuildInfo']['binaryUpdateSchema']==manifest['coreBuildInfo']['dataSchema']==1
for item in manifest['artifacts']:assert digest(package.parent/item['path'])==item['sha256'] and item['authenticodeStatus']=='NotSigned'
assert '8 passed; 0 failed' in log('backup-unit')
assert 'Finished' in log('clippy') and 'error:' not in log('clippy')
assert 'Finished 1 bundle' in log('package-build') and 'error:' not in log('package-build')
results={name:native(name) for name in ['native1','native2','native3','native4','native5','candidate']}
for name,result in results.items():
    assert load(reports/Path(result['run']).name/'result.json')==result
    if name in ['native1','native2']:assert result['status']=='failed' and result['cleanup']=='passed'
    else:assert result['status']=='passed'
passed=results['native5'];assert passed['cleanup']=='passed' and passed['actualProgramReplacement'] and passed['actualProgramRollback'] and passed['projectUnchanged']
assert passed['realProcessCrashTested'] and passed['crash']['processTerminated']
assert passed['crash']['journalState']=='apply-intent' and passed['crash']['observed']['installed']=='source'
assert not passed['schemaMigration'] and passed['modelCalls']==passed['userDesktopActions']==0
assert results['candidate']['passed']==10
runtime_hashes={name:digest(root/name) for name in ['target/release/cerulean-core.exe','target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']}
assert set(runtime_hashes.values())=={manifest['coreSha256']}
stamp=datetime.now().astimezone().isoformat()
body='''098 新增独立维护脚本，连接原候选包审查、hold和绑定备份，完成真实0.1.1安装到0.1.2双程序更新、提交前回滚及明确提交。原卸载程序/资源保留，只替换桌面和核心并更新原当前用户安装登记版本；不是通用NSIS迁移。工具仍在工程目录，未随安装包分发/接入UI。候选要求binaryUpdateSchema1/dataSchema1，源0.1.1为明确旧格式基线，版本严格前进；未改变活动账本格式。

Prepare绑定源/候选清单、原安装/数据/SID/hold、原及暂存全部程序清单、数据备份摘要；实际源程序和hold核心摘要必须相符。两包重新核验并持续持有句柄；准备阶段复制原程序资源，替换的两文件核对候选完整摘要。Apply一次性持久intent，原程序移入previous-program，再发布暂存目录，核验后保持installed-held。Rollback保留失败程序、恢复原目录及登记，hold保持。Commit先写intent再严格release，不自动启动服务或项目。计划摘要、事件摘要链、顺序/状态转换、同用户登记与互不重叠同卷路径均校验；独占原零字节writer.lock，每次变更核对完整备份和当前数据。

事件/计划采用唯一临时文件写入Flush后禁止覆盖地发布，部分临时记录保留不读取。Review增加实际目录身份观察，未知目录不覆盖。Windows实际最小实验确认子文件共享DELETE仍阻止父目录改名，故单次改名前关闭本工具程序句柄、改名后重新取得；并发启动可使维护拒绝，不杀用户程序、不自动重试，不能声称全窗口排除外部启动。

最终Windows PowerShell5真实更新/回滚/提交通过，版本信息与登记匹配，新版显式重开原项目状态不变。错误计划、非法事件、暂存程序/备份篡改、另一测试profile核心占用、重复操作与提交后回滚均拒绝。两个受控目录中断现场明确回滚通过。另一次真实维护进程终止：观察完整apply-intent后终止本次进程并等待退出；实测发生在目录移动前，重开看到原程序/候选暂存，拒绝重放Apply，明确回滚通过。退出码字段null，不解释为具体数值；后续取得独占锁和退出等待支持原进程已停止。其余改名窗口仍是受控现场，不代证完整进程崩溃/断电矩阵。

候选包新增句柄持有/释放检查，共10项；备份专项8项、格式化、严格workspace全targets clippy、完整release/前端/NSIS构建通过。0.1.2三项包文件均NotSigned、releaseAccepted=false，release三处runtime一致；debug仍为旧开发构建。没有重复旧模型接入/全部核心检查，继承未改路径。原型/097冻结证据保持。新NSIS包本批未重新做干净安装，真实测试从已验收0.1.1安装开始，最终原卸载器清理安装/登记通过。真实模型/用户桌面动作0，WorkBuddy064不重发。

保留失败：构建前Python默认GBK读取Rust失败，改UTF-8后从剩余步骤继续；首个实机检查因NSIS的InstallLocation实际带引号而拒绝，按生成模板同时核对UninstallString修正；第二次被自有子句柄阻止目录改名，独立实测定位后修复，两次测试均完整清理。native3/4为中间通过，native5为最终含实际终止结果。

下一步优先补commit-intent后丢失release ACK的只读对账/收口：现已拒绝重发Commit/Apply/Rollback，但没有专门消除该不确定状态。继续其余真实进程/磁盘满/断电、工具分发和升级UI、活动数据格式门槛及迁移、签名/更新服务、干净目标。全部原首批API/CLI/Agent/两桌面、真实OpenAI主管/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察保持。完整P3/P4/P5/P6未通过，目标active。签名证书问题仍待答复，不阻止其他工作；不关机、不关闭用户程序，无提交推送。

[更新事务说明](WINDOWS-UPDATE-TRANSACTION-V1.md)；[098验收索引](../reports/p6-transaction-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8');assert '卷002第006项（097）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第007项（098），progress，同格式程序更新/回滚及一个真实维护进程终止通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方097为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md';assert '第007项 / 098' not in volume.read_text(encoding='utf8')
handoff_body=body.replace('(WINDOWS-UPDATE-TRANSACTION-V1.md)','(../../docs/WINDOWS-UPDATE-TRANSACTION-V1.md)').replace('(../reports/','(../../reports/')
with volume.open('a',encoding='utf8') as f:f.write('\n\n## 第007项 / 098 程序更新事务与回滚\n\n'+stamp+'\n\n'+handoff_body)
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
matrix.write_text(matrix.read_text(encoding='utf8').replace('本页097小节','本页098小节')+'\n\n## 098 程序更新与回滚（限定范围组件通过）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';old_gates=gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 098：同格式0.1.1→0.1.2真实双程序更新、提交前回滚、明确提交及一个维护进程终止检查通过。独立工具未分发/接UI，完整P3/P4/P5/P6未通过。见[更新事务](WINDOWS-UPDATE-TRANSACTION-V1.md)、[最新交接](HANDOFF-LATEST.md)。

- 下一步：commit-intent后release ACK丢失的只读对账/明确收口；当前拒绝重发，但还不能自动判定提交完成。原程序/失败现场与临时记录均保留。
- 其余真实进程/断电/磁盘满窗口、并发启动/干净目标、工具分发和升级UI继续补齐；本批实际终止发生在程序移动前，两个后续目录边界是受控夹具。
- 活动数据格式门槛和迁移未完成，本批固定同格式程序更新保留原卸载器/资源，不是通用NSIS迁移。提交后不允许该事务恢复旧程序。
- 0.1.2包未签名；签名/更新服务与证书身份仍待完成。debug主程序仍为旧开发构建，不与release混称。
- 全部原首批接入、真实OpenAI管理/编程/含失败效率、两桌面/共享故障负载、金额预算/按执行者监督及075/085观察保持。先查原型，不重发WorkBuddy064。本日卷002第007项，旧冻结证据保持。

'''+old_gates[old_gates.index('以下为较早阶段的历史记录。'):],encoding='utf8')
paths={root/r['source'] for r in previous['records'] if r['source'].startswith('crates/')}
for name in ['Cargo.toml','Cargo.lock','package.json','package-lock.json','apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/installer/hooks.nsh','scripts/desktop.mjs','scripts/windows-package-image.ps1','scripts/freeze-windows-package.ps1','scripts/test-windows-package.ps1','scripts/check-package-candidate-20261012.ps1','scripts/manage-windows-update.ps1','scripts/check-update-transaction-20261012.ps1','scripts/finalize-transaction-20261012.py','docs/WINDOWS-UPDATE-TRANSACTION-V1.md','docs/UPDATE-CANDIDATE-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','README.md']:
    paths.add(root/name)
paths.add(volume);paths.add(package);paths.update(reports.glob('p6-transaction-*-20261012.log'));paths.add(reports/'p6-transaction-rename-guard-probe-20261012.json')
for result in results.values():
    run=reports/Path(result['run']).name;paths.add(run/'result.json')
    # Synthetic test transaction plans/events contain no credentials; program/data bytes stay in the original run.
    for p in run.glob('transaction-*/*.json'):paths.add(p)
    for p in run.glob('interrupted-updater.*'):paths.add(p)
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=digest(p);assert digest(dest)==value;records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'098','at':stamp,'status':'same-schema-binary-update-rollback-and-process-interruption-verified','goalStatus':'active',
    'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'crossVersionBinaryUpdateTested':True,'ledgerSchemaMigrationTested':False,
    'realModelCalls':0,'userDesktopActions':0,'backupUnitPassed':8,'candidateChecks':10,'nativeResult':passed,'intermediateResults':{k:v for k,v in results.items() if k!='native5'},
    'packageManifest':package.relative_to(root).as_posix(),'packageManifestSha256':digest(package),'packageArtifacts':manifest['artifacts'],'releaseRuntimeHashes':runtime_hashes,
    'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'oldPackagePreserved':True,
    'remaining':['commit ACK uncertainty reconciliation','full crash/disk-full/power-loss matrix','tool distribution/upgrade UI','active data format gates/migration','signing/update service/clean target','full original real-provider/desktop/coding/efficiency scope'],
    'archiveScope':'selected source and synthetic evidence; not a complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'packageManifestSha256':digest(package),'goalStatus':'active'},ensure_ascii=False))
