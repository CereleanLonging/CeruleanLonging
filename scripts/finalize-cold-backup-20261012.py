"""Freeze the new maintenance component; retain original scope and old evidence."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging')
reports = root / 'reports'
index = reports / 'p6-cold-backup-20261012.json'
archive = reports / 'p6-cold-backup-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(stage):
    raw = (reports / f'p6-cold-backup-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous_path = reports / 'p3-workbench-cli-20261012.json'
previous = load(previous_path)
for record in previous['records']:
    assert digest(root / record['snapshot']) == record['sha256'], record['snapshot']
    if record['source'].startswith('crates/'):
        if record['source'] == 'crates/core/src/lib.rs':
            current = (root / record['source']).read_text(encoding='utf8')
            old = (root / record['snapshot']).read_text(encoding='utf8')
            assert current.replace('#[cfg(windows)]\npub mod backup;\n', '', 1) == old
        elif record['source'] != 'crates/core/src/workbench_cli.rs':
            assert digest(root / record['source']) == record['sha256'], record['source']
assert '8 passed; 0 failed' in log('unit3')
assert '4 passed; 0 failed' in log('cli')
assert '7 passed; 1 failed' in log('unit1')  # preserve the actual overwrite discovery
for stage in ['build2', 'clippy2', 'desktop']:
    assert 'Finished' in log(stage) and 'error:' not in log(stage)
native = json.loads([line for line in log('native').splitlines() if line.startswith('{')][-1])
assert native['status'] == 'passed' and native['byteExactRestore'] and native['workflowUnchanged']
assert native['restoreDidNotStartService'] and native['liveCoreRejected']
assert native['realModelCalls'] == 0 and native['userDesktopActions'] == 0
native_folder = reports / Path(native['report']).name
assert load(native_folder / 'result.json') == native
runtimes = {name: digest(root / name) for name in previous['runtimeHashes']}
assert len(set(runtimes.values())) == 1
stamp = datetime.now().astimezone().isoformat()
body = f'''094 实现原方案第12节要求的核心数据离线备份环节，新增 --workbench backup create/verify/restore。直接获取原根 Store 写锁及嵌套 writer.lock，不启动核心，不引入第二账本。保存普通文件、空目录、零字节锁、原批准/事件/日志和 DPAPI 凭证密文；清单记录每项路径、类型、长度、SHA256。文件句柄禁止并发写入和替换，拒绝硬链接、目录内重解析点、损坏/缺失/多余内容及非法清单。恢复要求已审阅清单摘要、原 Windows SID、原规范化路径和不存在的目标目录；不覆盖、不合并、不自动启动或恢复任务。

通过8项新增 Rust 专项：原 Store 已批准计划重开、原位置 DPAPI 解密、未知状态原始记录/空目录保留，根及嵌套写锁冲突、损坏/缺失/多余/不完整备份、路径越界/重复/格式拒绝、错误用户/位置/摘要、硬链接、持有句柄期间写入/改名拒绝，以及发布时现有目录保护。未知记录检查是字节保留，不代证所有工作流恢复语义。实际 CLI/常驻核心使用新隔离目录验证：中文空格路径、真实单写锁、合成凭证、5个数据文件逐字节恢复、重开项目记录不变、恢复后服务未自动启动。只有本次测试核心启动/停止，无真实模型调用、无用户桌面动作。

首次专项7通过/1失败，发现 std::fs::rename 实际会替换已存在空目录。已改为 MoveFileExW、不带替换或跨卷复制标志，重新8项通过。首次严格 clippy 提示设备名布尔判断写法不清楚，改为等价布尔式后专项及严格检查通过。全部旧失败日志保留。文件同步与暂存发布不等于物理断电保证；失败暂存保留，不自动清理或重试。

本批检查：新增8项、原CLI单元4项、实际CLI备份恢复、严格workspace/all-targets clippy、核心与桌面custom-protocol debug构建。原核心305项、非核心156项、Node5项与前端沿093/092/091既有证据继承；未重复基础接入，未宣称本批重跑全部核心。复核093的{len(previous['records'])}份冻结归档；原Rust除CLI和lib模块声明外保持。三个runtime摘要一致：{next(iter(runtimes.values()))}。

本功能仅备份核心数据，不包含外部项目/worktree/验收器/第三方桌面历史。恢复旧快照可能缺少备份后的外部派发记录，不能据此自动重跑；本版不提供活跃项目自动历史回滚。尚缺跨路径/用户/机器迁移、数据格式转换、升级失败恢复、旧版本降级兼容、物理断电/磁盘满完整矩阵、安装卸载、签名与支持矩阵。完整P3/P4/P5/P6仍未验收，目标active。真实OpenAI主副指挥、全部首批API/CLI/桌面、真实编程及含失败效率、共享故障负载、金额硬预算/分执行者监督策略、075/085未定观察全部保留；WorkBuddy064单次任务不重发。

[冷备份说明](COLD-BACKUP-V1.md)；[094验收索引](../reports/p6-cold-backup-20261012.json)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf8')
assert '卷002第002项（093）' in old.splitlines()[2]
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第003项（094），progress，094最终检查已通过。\n\n'+stamp+'\n\n'+body+'\n下方093为冻结历史。\n\n'+old.removeprefix('# 最新交接\n\n'), encoding='utf8')
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第003项 / 094' not in volume.read_text(encoding='utf8')
volume_body = body.replace('(COLD-BACKUP-V1.md)', '(../../docs/COLD-BACKUP-V1.md)').replace('(../reports/', '(../../reports/')
with volume.open('a', encoding='utf8') as f:
    f.write('\n\n## 第003项 / 094 核心数据冷备份与原位置恢复\n\n'+stamp+'\n\n'+volume_body)
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md'
text = matrix.read_text(encoding='utf8').replace('本页093小节', '本页094小节')
matrix.write_text(text+'\n\n## 094 冷备份与原位置恢复（组件通过，完整P6未通过）\n\n'+stamp+'\n\n'+body, encoding='utf8')
gates = root / 'docs/NEXT-STAGE-GATES.md'
text = gates.read_text(encoding='utf8')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 094 更新：冷备份、完整清单校验、同用户原位置恢复通过新增专项和实际CLI。复用原单写锁、账本及DPAPI密文，无自动启动任务。见[冷备份说明](COLD-BACKUP-V1.md)、[最新交接](HANDOFF-LATEST.md)。完整P3/P4/P5/P6未通过。

- 下一步补可使用的安装/卸载与明确版本清单，随后将冷备份接入升级失败恢复；跨路径迁移、格式转换、旧版回滚、签名和干净机器支持矩阵仍需独立验收。备份组件不能代替完整P6。
- 真实OpenAI主副指挥、全部首批API/CLI/桌面、真实编程和含失败效率继续保留。已验证接入先查预案，WorkBuddy064不重发。
- 共享故障/负载完整矩阵、金额硬预算、按执行者监督策略、075/085未定观察仍保留。原schema2/binding2不能静默交给旧程序。
- 本日卷002当前第003项，旧冻结证据保留。

'''+text[text.index('以下为较早阶段的历史记录。'):], encoding='utf8')
names = ['crates/core/src/backup.rs','crates/core/src/lib.rs','crates/core/src/workbench_cli.rs',
         'tests/cold-backup.test.mjs','scripts/check-cold-backup-20261012.ps1','scripts/finalize-cold-backup-20261012.py',
         'README.md','docs/COLD-BACKUP-V1.md','docs/WORKBENCH-CLI-V1.md','docs/HANDOFF-LATEST.md',
         'docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','Cargo.toml','Cargo.lock','crates/core/Cargo.toml']
paths = {root / name for name in names}
paths.add(volume)
paths.update(reports.glob('p6-cold-backup-*-20261012.log'))
paths.add(native_folder / 'result.json')  # no credential blobs or data payload archived
archive.mkdir()
records = []
for path in sorted(paths):
    relative = path.relative_to(root)
    dest = archive / relative
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, dest)
    value = digest(path)
    assert digest(dest) == value
    records.append({'source':relative.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report = {'batch':'094','at':stamp,'status':'cold-backup-original-location-restore-verified','goalStatus':'active',
          'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
          'realModelCalls':0,'userDesktopActions':0,'newBackupUnitPassed':8,'cliUnitPassed':4,
          'inheritedCorePassed':305,'inheritedNonCoreRustPassed':156,'inheritedNodePassed':5,
          'runtimeHashes':runtimes,'nativeResult':native,
          'previousIndex':previous_path.relative_to(root).as_posix(),'previousIndexSha256':digest(previous_path),
          'previousArchiveVerified':len(previous['records']),
          'remaining':['installation/uninstallation','upgrade failure restore and version compatibility','cross-location/machine migration','rollback/signing/support matrix','original real-provider/programming/efficiency and shared-fault scope'],
          'archiveScope':'selective sources and evidence; inherited source hashes in previous index; no credential blobs; not a full reproducible repository',
          'records':records}
with index.open('x',encoding='utf8') as f:
    json.dump(report,f,ensure_ascii=False,indent=2); f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'runtimeSha256':next(iter(runtimes.values()))}, ensure_ascii=False))
