"""Freeze batch 097; never rewrite a previous evidence batch or package."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json, shutil
root=Path(r'\\?\D:\test\CeruleanLonging');reports=root/'reports'
index=reports/'p6-candidate-20261012.json';archive=reports/'p6-candidate-sources-20261012'
assert not index.exists() and not archive.exists(),'Preserve frozen evidence'
def digest(p):return sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def log(stage):
    raw=(reports/f'p6-candidate-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
def native(stage):return json.loads([x for x in log(stage).splitlines() if x.startswith('{')][-1])
previous_path=reports/'p6-upgrade-20261012.json';previous=load(previous_path)
for item in previous['records']:assert digest(root/item['snapshot'])==item['sha256'],item['snapshot']
changed={'crates/core/src/lib.rs','crates/core/src/main.rs','crates/core/src/backup.rs'}
for item in previous['records']:
    if item['source'].startswith('crates/') and '/src/' in item['source'] and item['source'] not in changed:
        assert digest(root/item['source'])==item['sha256'],item['source']
old_package=reports/'p6-windows-package-final-20261012/manifest.json'
assert digest(old_package)=='8a60fac043f9ddc2e8793a8fe7d7617a252ae1d5ba666c38ec3e37aa285e38e3'
for item in load(old_package)['artifacts']:assert digest(old_package.parent/item['path'])==item['sha256']
assert '8 passed; 0 failed' in log('backup-unit')
assert 'Finished' in log('clippy') and 'error:' not in log('clippy')
assert 'Finished 1 bundle' in log('package-build') and 'error:' not in log('package-build')
results={name:native(name) for name in ['version-native','review-native','review-ps5','installer-native']}
for name,result in results.items():
    assert result['status']=='passed',name
    assert result['realModelCalls']==0 and result['userDesktopActions']==0
    assert load(reports/Path(result['run']).name/'result.json')==result
assert results['version-native']['oldBackupReadAndRestored'] and results['version-native']['oldBackupReaderRejectsNewWriter']
assert not results['version-native']['ledgerSchemaMigrationTested']
assert results['review-native']['passed']==results['review-ps5']['passed']==8
assert results['installer-native']['installedHashesMatch'] and not results['installer-native']['cleanMachine']
package=reports/'p6-windows-package-011-20261012/manifest.json';manifest=load(package)
assert digest(package)=='604a287ca46d47ef6509fed3592a4182802eb6c669b59147383501c0d9b2ad1a'
assert manifest['version']=='0.1.1' and not manifest['releaseAccepted']
for item in manifest['artifacts']:assert digest(package.parent/item['path'])==item['sha256'] and item['authenticodeStatus']=='NotSigned'
runtime_hashes={name:digest(root/name) for name in ['target/release/cerulean-core.exe','target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']}
assert set(runtime_hashes.values())=={manifest['coreSha256']}
baseline=reports/'p6-candidate-baseline-20261012'
assert digest(baseline/'cerulean-runtime-0.1.0.exe')==load(baseline/'provenance.json')['runtimeSha256']==results['version-native']['oldRuntimeSha256']
stamp=datetime.now().astimezone().isoformat()
body='''097 将工程版本一致推进到0.1.1，依赖版本保持锁定，生成包含096升级准备能力的内部release/NSIS包。核心增加无Store、IPC、模型操作的 --build-info，返回平台、协议、hold/backup格式与明确的备份写入版本。冻结清单保存实际能力，核对三处release runtime摘要；旧095包和096证据保持。

候选包审查要求已审阅清单摘要，固定三项文件名称，检查长度/摘要、实际Authenticode及运行时能力。持有普通文件和逐级目录句柄，拒绝重解析点/硬链接，核对后只运行有时限与输出上限的 --build-info。默认发布路径需要已批准清单与预期发布者指纹及实际有效签名；当前只能显式选择内部未签名通道。摘要不证明发布者，查询不是不可信程序沙箱；脚本结束释放句柄，后续安装事务必须重新固定核对，不能凭旧结果直接安装。

原备份按当前版本精确比较，版本更新会拒绝旧备份；现限定schema1和明确writer列表0.1.0/0.1.1。真实使用保存的096核心0.1.0生成备份，新release0.1.1核验并在同用户原路径恢复五个文件，逐字节一致、锁定保留，显式解除后原项目不变。旧读取器拒绝新版备份，新版拒绝未知版本/schema。空工作目录能力查询无文件副作用。只证明备份容器兼容，不证明旧程序拒绝新版活动数据或账本迁移。

备份专项8项、格式检查、严格workspace全targets clippy通过；完整前端/release/NSIS构建通过。候选审查8场景分别在当前PowerShell和Windows PowerShell5通过；旧包缺能力、摘要/二进制篡改、能力不符、非固定路径、默认未签名和伪称已发布都拒绝。新版包实际隔离安装后payload摘要相符；原核心运行时重装/卸载返回20且PID/文件保持，停止本次测试核心后卸载成功且隔离/默认数据摘要不变，测试注册清理。无真实模型、用户桌面动作或WorkBuddy重发。096原320项核心/155忽略与IPC退出屏障等未改路径继承，没有重复所有检查或接入。

冻结包 reports/p6-windows-package-011-20261012，清单SHA256 604a287ca46d47ef6509fed3592a4182802eb6c669b59147383501c0d9b2ad1a；三项文件均NotSigned、releaseAccepted=false。release runtime与打包sidecar为0.1.1；debug主程序仍保留096版本，下次开发构建需正常重建，不能混称为同一构建。发布者证书问题已询问、尚无回复，不阻止无证书的内部工作。

下一步实现持久升级事务及程序失败恢复，连接候选包/hold/backup，核对真实程序替换、旧代码/新活动数据格式门槛和回滚。升级UI、签名更新服务、干净机器、断电/磁盘满与迁移完整矩阵仍待验收。原全部首批API/CLI/Agent/两桌面、真实OpenAI主副指挥/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085未定观察均保持。完整P3/P4/P5/P6未通过，目标active。WorkBuddy064一次授权已执行完毕，不重发sum任务；不关机、不关闭用户程序，无提交推送。

[更新包说明](UPDATE-CANDIDATE-V1.md)；[097验收索引](../reports/p6-candidate-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8');assert '卷002第005项（096）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第006项（097），progress，0.1.1候选包与备份版本兼容组件通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方096为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md';assert '第006项 / 097' not in volume.read_text(encoding='utf8')
with volume.open('a',encoding='utf8') as f:
    handoff_body=body.replace('(UPDATE-CANDIDATE-V1.md)','(../../docs/UPDATE-CANDIDATE-V1.md)').replace('(../reports/','(../../reports/')
    f.write('\n\n## 第006项 / 097 候选包与备份版本兼容\n\n'+stamp+'\n\n'+handoff_body)
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
matrix.write_text(matrix.read_text(encoding='utf8').replace('本页096小节','本页097小节')+'\n\n## 097 候选包与备份版本兼容（组件通过）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';old_gates=gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 097：0.1.1内部包包含升级准备，候选包完整性/实际能力检查、跨版本备份读取及本机隔离安装通过。完整P3/P4/P5/P6未通过。见[候选包说明](UPDATE-CANDIDATE-V1.md)和[最新交接](HANDOFF-LATEST.md)。

- 下一步实现持久升级执行事务，重新固定核对候选程序，连接原hold/backup/恢复，实际替换程序及失败回滚。不能将审查脚本当作已经完成升级执行器。
- 当前仅验证schema1备份容器writer0.1.0/0.1.1兼容。活动数据版本门槛、旧代码拒绝新格式、账本迁移仍未完成；095旧包不支持hold。
- 冻结0.1.1包三项文件未签名；证书身份待用户答复。签名成功路径、升级UI/更新服务、干净目标、磁盘满/断电/迁移矩阵保持。
- 全部首批接入、真实OpenAI管理/编程/含失败效率、两桌面/共享故障负载、金额预算/按执行者监督及075/085观察保持。先查原型证据；WorkBuddy064不重发。
- 本日卷002第006项；release与sidecar已为0.1.1，旧debug仍为096基线。旧冻结包及证据不改写。

'''+old_gates[old_gates.index('以下为较早阶段的历史记录。'):],encoding='utf8')
paths={root/r['source'] for r in previous['records'] if r['source'].startswith('crates/')}
for name in ['Cargo.toml','Cargo.lock','package.json','package-lock.json','apps/desktop/src-tauri/tauri.conf.json','scripts/desktop.mjs','scripts/windows-package-image.ps1','scripts/freeze-windows-package.ps1','scripts/test-windows-package.ps1','scripts/check-package-candidate-20261012.ps1','scripts/check-windows-installer-20261012.ps1','scripts/finalize-candidate-20261012.py','tests/backup-version.test.mjs','docs/UPDATE-CANDIDATE-V1.md','docs/UPGRADE-PREPARATION-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','README.md']:
    paths.add(root/name)
paths.add(volume);paths.add(package);paths.add(baseline/'provenance.json');paths.update(reports.glob('p6-candidate-*-20261012.log'))
for result in results.values():paths.add(reports/Path(result['run']).name/'result.json')
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=digest(p);assert digest(dest)==value;records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'097','at':stamp,'status':'candidate-and-backup-compatibility-verified','goalStatus':'active',
    'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'crossVersionBinaryUpgradeTested':False,'ledgerSchemaMigrationTested':False,
    'realModelCalls':0,'userDesktopActions':0,'backupUnitPassed':8,'candidateChecksPerHost':8,'candidateHosts':['current PowerShell','Windows PowerShell 5'],
    'inheritedCorePassed':320,'inheritedCoreIgnored':155,'packageManifest':package.relative_to(root).as_posix(),'packageManifestSha256':digest(package),
    'packageArtifacts':manifest['artifacts'],'releaseRuntimeHashes':runtime_hashes,'results':results,
    'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),'oldPackagePreserved':True,
    'remaining':['upgrade transaction and binary rollback','active data format gates/migration','signing and update service','clean target/power loss/disk full/migration','full original real-provider/desktop/coding/efficiency scope'],
    'archiveScope':'selected source/evidence, not complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'packageManifestSha256':digest(package),'goalStatus':'active'},ensure_ascii=False))
