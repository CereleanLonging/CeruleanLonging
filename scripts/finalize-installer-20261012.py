"""Freeze the tested internal installer without claiming full P6 release."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root=Path(r'\\?\D:\test\CeruleanLonging')
reports=root/'reports'
index=reports/'p6-installer-20261012.json'
archive=reports/'p6-installer-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def digest(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def log(stage):
    raw=(reports/f'p6-installer-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
previous_path=reports/'p6-cold-backup-20261012.json'
previous=load(previous_path)
for r in previous['records']:
    assert digest(root/r['snapshot'])==r['sha256'],r['snapshot']
    if r['source'].startswith('crates/'):
        assert digest(root/r['source'])==r['sha256'],r['source']
older=load(reports/'p3-workbench-cli-20261012.json')
for r in older['records']:
    assert digest(root/r['snapshot'])==r['sha256'],r['snapshot']
    if r['source'].startswith('crates/') and r['source'] not in {'crates/core/src/lib.rs','crates/core/src/workbench_cli.rs'}:
        assert digest(root/r['source'])==r['sha256'],r['source']
assert 'Finished 1 bundle' in log('build4') and 'built in' in log('build4')
assert 'Finished 1 bundle' in log('bundle5')
native=json.loads([s for s in log('native3').splitlines() if s.startswith('{')][-1])
assert native['status']=='passed'
for key in ['installAndUninstall','busyReinstallRejected','busyUninstallRejected','corePidUnchanged','installedHashesMatch','standardDataUnchanged','isolatedDataUnchanged']:
    assert native[key],key
assert [c['exit'] for c in native['calls']]==[0,20,20,0]
guard=load(reports/'nsis-delete-guard-20261012/result.json')
assert guard['status']=='passed' and guard['exit']==21 and not guard['maintenanceReached']
assert guard['hookSha256']==digest(root/'apps/desktop/src-tauri/installer/hooks.nsh')
package=reports/'p6-windows-package-final-20261012'
manifest=load(package/'manifest.json')
assert manifest['releaseAccepted'] is False and manifest['downgradeAllowed'] is False
for entry in manifest['artifacts']:
    assert digest(package/entry['path'])==entry['sha256']
    assert (package/entry['path']).stat().st_size==entry['bytes']
    assert entry['authenticodeStatus']=='NotSigned'
for entry in manifest['buildInputs']:
    assert digest(root/entry['path'])==entry['sha256'],entry['path']
release_paths=['target/release/cerulean-core.exe','target/release/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
release_hashes={name:digest(root/name) for name in release_paths}
assert set(release_hashes.values())=={manifest['coreSha256']}
stamp=datetime.now().astimezone().isoformat()
body=f'''095 将原 --no-bundle 桌面构建扩展为独立 desktop:package 入口，完成 release 核心、契约、前端、桌面与NSIS。最终完整入口构建及钩子补丁再打包通过，Rust依赖使用--locked。Windows当前用户安装、简体中文/英语、禁止直接降级；WebView2仍沿Tauri引导方式。本机工具下载来自锁定Tauri CLI指定地址并经其哈希校验；不是安装第三方Agent。NSIS产物已冻结，三项交付文件实际Authenticode均NotSigned，releaseAccepted=false。

审查实际生成模板发现默认静默流程可能强制关闭主程序。新增安装钩子覆盖CheckIfAppIsRunning，桌面或核心映像被占用则退出20，不终止进程；维护期间持有默认数据目录已有writer.lock的独占文件句柄。默认卸载保留数据，选中上游删除数据选项则立即退出21，提示取消勾选；独立清理/备份审阅尚未实现，不允许递归删除权威账本。原数据、用户程序和WorkBuddy064单次消息均不重新操作。

最终包实际隔离安装通过：包内桌面/核心完整哈希一致，安装后实际核心运行；重装与卸载都返回20，原核心PID与安装文件保持；明确停止本次测试核心后卸载返回0，程序及卸载注册移除，默认真实数据目录和测试目录摘要不变。仅清理测试前不存在且仍指向本次目录的产品注册项。未启动产品界面；非干净Windows机器。实际NSIS夹具使用同一卸载钩子，选中删除状态返回21，维护副作用标记没有创建；这不是交互页面验收。

失败证据保留：首个Node构建被沙箱EPERM阻止启动Rust，原工具正常宿主构建成功，没有改产品权限。首次安装核验发现桌面文件与构建输入仅差3字节：Tauri在包内将唯一包类型UNK改为NSS并恢复构建输入。新增未签名映像准备函数，限定唯一已知标记，核对整个实际安装文件；签名文件拒绝变换。清理该测试安装后，修正检查和最终包均完整通过。额外makensis/PPO诊断三次在插件识别处未完成，未作为通过证据；实际完整打包与保护宏夹具编译/执行通过，不据此声称展开脚本检查完成。

本批构建前端类型检查和Vite/release桌面/核心/NSIS通过；原Rust调度、IPC、凭证和备份源码未改，继承093核心305项、CLI4项、非核心156项、Node5项及094冷备份8项，不重复基础接入或全量回归。复核094的{len(previous['records'])}份与093的{len(older['records'])}份冻结归档。release三处核心一致：{manifest['coreSha256']}；debug核心仍为094版本，不能把不同profile混成同一摘要。最终包的文件、版本、签名状态及源配置摘要见其manifest。

完整P3/P4/P5/P6仍未验收，目标active。后续优先接入升级前持久状态审阅、可校验备份、失败恢复与版本策略，完成干净目标机器/真实安装后界面、签名及更新服务。交互卸载页面、磁盘满/断电完整矩阵、回滚/转换/跨机器迁移仍缺。全部真实OpenAI主副指挥、六家API/选定CLI、Codex/OMP/Qoder/WorkBuddy原会话和真实编程、共享故障/负载、含失败效率、金额预算/按执行者监督策略、075/085未定观察保持原范围；不以安装组件通过代替全首批签收。

[安装包说明](WINDOWS-INSTALLER-V1.md)；[最终内部包清单](../reports/p6-windows-package-final-20261012/manifest.json)；[095验收索引](../reports/p6-installer-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8')
assert '卷002第003项（094）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第004项（095），progress，安装包组件验收通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方094为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第004项 / 095' not in volume.read_text(encoding='utf8')
with volume.open('a',encoding='utf8') as f:
    f.write('\n\n## 第004项 / 095 Windows安装包与维护保护\n\n'+stamp+'\n\n'+body.replace('(WINDOWS-INSTALLER-V1.md)','(../../docs/WINDOWS-INSTALLER-V1.md)').replace('(../reports/','(../../reports/'))
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
text=matrix.read_text(encoding='utf8').replace('本页094小节','本页095小节')
matrix.write_text(text+'\n\n## 095 NSIS安装包（组件通过，完整P6未通过）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';text=gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 095 更新：Windows NSIS实际隔离安装/卸载、运行中拒绝维护、原数据保留、精确包映像及版本清单通过。包未签名，未在干净机器启动产品界面，完整P3/P4/P5/P6均未通过。见[安装包说明](WINDOWS-INSTALLER-V1.md)、[最新交接](HANDOFF-LATEST.md)。

- 下一步优先连接升级前持久状态审阅、冷备份与失败恢复；安装器当前只拒绝运行占用，不代证未知桌面任务迁移。数据版本、降级回滚、跨机器转换保持独立验收。
- 完成签名身份/更新服务、干净目标和WebView2支持矩阵、安装后真实界面及业务闭环；上游删除数据选项被21拒绝，独立清理及交互页面待完成。
- 全部真实OpenAI主副指挥、首批API/CLI/Agent/两桌面、真实编程及含失败效率、共享故障负载、金额预算/分执行者监督与075/085未定观察仍保留。原接入先查预案；WorkBuddy064不重发。
- 本日卷002当前第004项。旧冻结产物保留，debug与release摘要分别记录，不能宣称完整P6已通过。

'''+text[text.index('以下为较早阶段的历史记录。'):],encoding='utf8')
names=['scripts/desktop.mjs','scripts/windows-package-image.ps1','scripts/freeze-windows-package.ps1',
       'scripts/check-windows-installer-20261012.ps1','scripts/check-installer-delete-guard-20261012.ps1','scripts/finalize-installer-20261012.py',
       'apps/desktop/src-tauri/tauri.conf.json','apps/desktop/src-tauri/installer/hooks.nsh','package.json','package-lock.json','Cargo.toml','Cargo.lock',
       'docs/WINDOWS-INSTALLER-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','README.md',
       'target/release/nsis/x64/installer.nsi','target/release/nsis/x64/utils.nsh']
paths={root/n for n in names};paths.add(volume)
paths.update(reports.glob('p6-installer-*-20261012.log'))
paths.update(reports.glob('p6-installer-preprocessed*-20261012.nsi'))
paths.add(package/'manifest.json')
for folder in reports.glob('installer-native-*'):
    paths.update(folder.glob('*.json'))
for name in ['guard.nsi','compile.log','result.json']:
    paths.add(reports/'nsis-delete-guard-20261012'/name)
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel
    dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=digest(p);assert digest(dest)==value
    records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'095','at':stamp,'status':'internal-nsis-package-and-maintenance-guards-verified','goalStatus':'active',
        'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
        'cleanMachineVerified':False,'signedRelease':False,'realModelCalls':0,'userDesktopActions':0,
        'nativeResult':native,'deleteGuardResult':guard,'packageManifest':(package/'manifest.json').relative_to(root).as_posix(),
        'packageManifestSha256':digest(package/'manifest.json'),'releaseRuntimeHashes':release_hashes,
        'inheritedCoreTests':305,'inheritedBackupTests':8,'inheritedCliTests':4,'inheritedNonCoreTests':156,'inheritedNodeTests':5,
        'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),
        'remaining':['clean target installation and product UI','upgrade state review/backup/failure recovery','format conversion and downgrade/migration','signing/update service/support matrix','full original P3-P5 provider/desktop/coding/efficiency/fault scope'],
        'archiveScope':'selective evidence and build inputs; product binaries in frozen package directory; not a complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f:
    json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'package':str(package),'goalStatus':'active'},ensure_ascii=False))
