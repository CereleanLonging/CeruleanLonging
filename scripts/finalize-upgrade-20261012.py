"""Freeze quiescence/backup evidence, preserving all original release gates."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json,re,shutil
root=Path(r'\\?\D:\test\CeruleanLonging');reports=root/'reports'
index=reports/'p6-upgrade-20261012.json';archive=reports/'p6-upgrade-sources-20261012'
assert not index.exists() and not archive.exists(),'Preserve evidence'
def digest(p):return sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text(encoding='utf-8-sig'))
def log(stage):
    raw=(reports/f'p6-upgrade-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
previous_path=reports/'p6-installer-20261012.json';previous=load(previous_path)
for r in previous['records']:assert digest(root/r['snapshot'])==r['sha256'],r['snapshot']
package=root/previous['packageManifest'];assert digest(package)==previous['packageManifestSha256']
for e in load(package)['artifacts']:assert digest(package.parent/e['path'])==e['sha256']
base=load(reports/'p3-workbench-cli-20261012.json')
changed={'crates/core/src/lib.rs','crates/core/src/ipc.rs','crates/core/src/workbench_cli.rs','crates/core/src/desktop_cancellation_tests.rs'}
for r in base['records']:
    if r['source'].startswith('crates/') and r['source'] not in changed:
        assert digest(root/r['source'])==r['sha256'],r['source']
assert '320 passed; 0 failed; 155 ignored' in log('core3')
assert '6 passed; 0 failed' in log('unit2')
assert 'upgrade_partial_hold_rejects_direct_start_before_creating_any_store_files ... ok' in log('core3')
assert '4 passed; 0 failed' in log('cli2')
assert re.search(r'(?:#|ℹ) pass 5\b',log('ipc')) and re.search(r'(?:#|ℹ) fail 0\b',log('ipc'))
for stage in ['build3','clippy3','desktop3']:assert 'Finished' in log(stage) and 'error:' not in log(stage)
native=json.loads([s for s in log('native').splitlines() if s.startswith('{')][-1])
assert native['status']=='passed' and native['queuedRequestRejected'] and native['holdBoundBackup'] and native['restoredHoldRetained']
assert not native['crossVersionUpgradeTested']
folder=reports/Path(native['report']).name
assert load(folder/'result.json')==native
runtimes={name:digest(root/name) for name in ['target/debug/cerulean-core.exe','target/debug/cerulean-runtime.exe','apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']}
assert len(set(runtimes.values()))==1
stamp=datetime.now().astimezone().isoformat()
body=f'''096 开发构建新增 upgrade review/prepare/status/backup/release。审阅原完整快照、阻塞项、核心版本和实际二进制摘要；准备必须匹配当前状态摘要。沿原退出条件再检查诊断、未决发布/启动、全历史管理计数及桌面不确定记录。诊断允许退出不等于允许升级；已确认取消的桌面任务可以准备，Unknown仍阻止。准备持久化upgrade-hold.json并等待原认证进程句柄退出；原Store在创建目录前与取得原写锁后均核对锁定，当前启动器也拒绝持锁定记录启动，包括残缺记录。没有安装新版、没有自动恢复项目或授权模型调用。

修复原IPC退出竞态：检查、快照、停止标记现在位于同一ServiceState互斥锁，后到请求返回coreStopping，后台tick不再驱动执行。普通退出、诊断重启和停止复核重启统一使用该屏障；停机信号先于ACK写出，回执丢失仍停止。新可选升级字段仅在新操作响应出现，普通响应结构及原SID/ACL、请求限额和角色权限保持。

绑定备份在原根写锁和文件句柄持有期间再次核对有效锁定记录及摘要，沿094冷备份保存全部字节。备份恢复保留锁定，不自动启动。明确release要求当前摘要和独占写锁，把原记录以禁止覆盖的改名归档；包括经用户审阅的残缺记录，均不删除证据、不宣称新版本已验收。生成记录只保存行动/状态摘要，无Key和私有推理。

新增7项检查通过：直接启动在创建目录/写锁文件前拒绝残缺升级记录，持久锁定/迟到请求/明确解除，状态变化和错误协议，诊断退出与升级权限区分，绑定备份/恢复保留锁/残缺记录，断开内存传输注入ACK丢失仍发停机信号，以及沿原桌面夹具的Unknown到确认取消。代码复查发现最初会把已确认取消也当未提交任务阻塞，修正并补原夹具检查；没有重跑真实桌面。编译首次遗漏Client请求关联ID的新枚举分支，补齐后通过，失败日志保留。

实际新本机流程通过：两个已握手命名管道连接，确认退出后迟到项目登记被coreStopping拒绝，原进程正常退出；旧审阅摘要拒绝；CLI准备等待原核心退出；当前程序启动被锁定；错误hold摘要不能备份；故障维护标记仅写新测试目录，保留失败目录后原位置恢复全部5个数据文件，恢复后仍锁定；明确解除不自动启动，显式重开原项目保持。真实模型调用/用户桌面动作0，WorkBuddy064不重发。此实机流程无桌面任务，沿用最后取消条件细化前的通过证据；变化的取消路径由新增夹具及最终全核心覆盖。

最终320项核心/155项显式忽略、CLI4项、原Node IPC5项、新本机流程、严格workspace全targets clippy、核心及桌面custom-protocol debug构建通过。非核心156项和前端继承095/093未改部分；没有宣称所有显式忽略故障矩阵均在本轮重跑。debug三处runtime一致：{next(iter(runtimes.values()))}。095的{len(previous['records'])}份冻结证据及全部包文件摘要保持，旧NSIS包未更新。

重要边界：旧095程序不识别新锁定文件，不能将它启动到准备中的目录；本次保护只针对包含096能力的核心。更新包、待安装版本能力审查、跨版本/数据格式兼容、旧代码拒绝新格式和程序失败回滚仍待实现，不能把持久文件当作对任意旧程序的强制安全边界。升级界面、签名/更新服务、干净目标、物理断电/磁盘满完整矩阵和迁移仍待完成。全部原真实管理/首批接入/两桌面/真实编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085未定观察保持。完整P3/P4/P5/P6未验收，目标active。

[升级准备说明](UPGRADE-PREPARATION-V1.md)；[096验收索引](../reports/p6-upgrade-20261012.json)。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf8');assert '卷002第004项（095）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第005项（096），progress，升级准备组件通过，完整P6未通过。\n\n'+stamp+'\n\n'+body+'\n下方095为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'),encoding='utf8')
volume=root/'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md';assert '第005项 / 096' not in volume.read_text(encoding='utf8')
with volume.open('a',encoding='utf8') as f:f.write('\n\n## 第005项 / 096 升级准备与退出屏障\n\n'+stamp+'\n\n'+body.replace('(UPGRADE-PREPARATION-V1.md)','(../../docs/UPGRADE-PREPARATION-V1.md)').replace('(../reports/','(../../reports/'))
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md';text=matrix.read_text(encoding='utf8').replace('本页095小节','本页096小节')
matrix.write_text(text+'\n\n## 096 升级准备与退出屏障（组件通过）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';text=gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 096 更新：升级前审阅、持久锁定、绑定冷备份及明确解除通过新增专项和实际IPC。修复退出后排队请求继续执行的问题。完整P3/P4/P5/P6未通过。见[升级准备说明](UPGRADE-PREPARATION-V1.md)、[最新交接](HANDOFF-LATEST.md)。

- 下一步构建包含096能力的更新包和升级执行事务；核对目标版本能力、数据格式、失败恢复和程序回滚。095旧包不识别锁定文件，不可把它当作支持该协议的候选版本；旧代码拒绝新格式仍需专门实现和验证。
- 数据恢复仅证明同用户原位置全部字节恢复、锁定保留及明确解除，不代证跨版本升级失败恢复。升级界面、签名/更新服务、干净目标、磁盘满/断电/迁移矩阵仍保留。
- 原全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察保持原范围。先查原型证据，WorkBuddy064不重发。
- 本日卷002第005项。当前新命令仅在开发构建，旧冻结安装包保持；完整P6尚未通过。

'''+text[text.index('以下为较早阶段的历史记录。'):],encoding='utf8')
paths={root/r['source'] for r in base['records'] if r['source'].startswith('crates/')}
for name in ['crates/core/src/upgrade.rs','crates/core/src/backup.rs','scripts/check-upgrade-20261012.ps1','scripts/finalize-upgrade-20261012.py','tests/upgrade.test.mjs',
             'docs/UPGRADE-PREPARATION-V1.md','docs/WORKBENCH-CLI-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','README.md','Cargo.lock','Cargo.toml']:
    paths.add(root/name)
paths.add(volume);paths.update(reports.glob('p6-upgrade-*-20261012.log'));paths.add(folder/'result.json')
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=digest(p);assert digest(dest)==value;records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'096','at':stamp,'status':'upgrade-quiescence-hold-backup-verified','goalStatus':'active',
    'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,'crossVersionUpgradeTested':False,
    'realModelCalls':0,'userDesktopActions':0,'corePassed':320,'coreIgnored':155,'newUpgradeTests':7,'cliPassed':4,'nodeIpcPassed':5,
    'nativeResult':native,'debugRuntimeHashes':runtimes,'oldPackageUpdated':False,
    'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),
    'remaining':['capability-checked candidate package','upgrade transaction and binary rollback','old-reader/new-format rejection','signing/clean target/migration/power loss','full original real-provider/desktop/coding/efficiency scope'],
    'archiveScope':'selected source/evidence, not complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'debugRuntimeSha256':next(iter(runtimes.values())),'goalStatus':'active'},ensure_ascii=False))
