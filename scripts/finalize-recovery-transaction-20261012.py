"""Freeze 107 core/CLI evidence, with unshipped integration gates explicit."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-recovery-transaction-20261012.json'
archive = reports / 'p6-recovery-transaction-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous_path = reports / 'p6-migration-io-20261012.json'; previous = load(previous_path)
for entry in previous['records']:
    assert digest(root / entry['snapshot']) == entry['sha256']
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for entry in load(package_path)['artifacts']:
    assert digest(package_path.parent / entry['path']) == entry['sha256']
failed = json.loads(log(reports / 'p6-recovery-transaction-native1-20261012.log'))
assert failed['status'] == 'failed' and failed['cleanup'] == 'passed'
assert failed['calls'][-1]['args'][3:5] == ['recovery', 'review']
assert not failed.get('pending')
native = json.loads(log(reports / 'p6-recovery-transaction-native2-20261012.log'))
run = reports / Path(native['run']).name
assert load(run / 'result.json') == native
assert native['status'] == 'passed' and native['cleanup'] == 'passed'
assert [x['name'] for x in native['cases']] == ['apply', 'abandon', 'moved-fixture']
assert digest(root / 'target/debug/cerulean-core.exe') == native['developmentCoreSha256']
assert native['cases'][0]['controllerDidNotMoveOrDeleteDataDirectories']
assert not native['cases'][2]['controllerDidNotMoveOrDeleteDataDirectories']
assert not native['shippedReleaseAccepted'] and not native['liveMigrationFault']
workspace_text = log(reports / 'p6-recovery-transaction-workspace-20261012.log')
rows = [list(map(int, x)) for x in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', workspace_text)]
workspace = {'passed': sum(x[0] for x in rows), 'failed': sum(x[1] for x in rows), 'ignored': sum(x[2] for x in rows)}
assert workspace == {'passed': 497, 'failed': 0, 'ignored': 176}
assert '7 passed; 0 failed' in log(reports / 'p6-recovery-transaction-tests2-20261012.log')
followups = {}
for name in ['ipc', 'resident']:
    text = log(reports / f'p6-recovery-transaction-{name}-20261012.log')
    counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;', text)
    assert counts and sum(int(x[0]) for x in counts) > 0
    followups[name] = {'passed': sum(int(x[0]) for x in counts), 'ignored': sum(int(x[1]) for x in counts)}
for name in ['clippy2', 'build2']:
    assert 'Finished `dev` profile' in log(reports / f'p6-recovery-transaction-{name}-20261012.log')
assert (reports / 'p6-recovery-transaction-fmt-20261012.log').exists()
stamp = datetime.now().astimezone().isoformat()
body = '''107 新增开发版核心/CLI备份恢复事务，复用原有冷备份清单、文件句柄、单写锁和禁止覆盖的发布机制。review绑定原路径/SID、备份摘要、原有效hold及故障目录完整清单；prepare重新核对后在同级专用目录保存计划、操作锁和完整待恢复副本，校验成功才记录准备完成。未完成或损坏的准备目录保持启动保护，不自动清理。

apply先写并同步移动意图，再将完整原目录保留在事务内；核对原目录清单和待恢复副本，写恢复意图后禁止覆盖地发布到原位置。失败保留原意图及全部目录，明确status后可以显式继续同一事务；已完成的apply只读回结果。finish要求恢复字节精确、保留目录精确、原hold有效和格式可读，才归档事务保护目录，仍不解除hold或启动核心。abandon仅允许尚未产生移动意图且原目录未变的准备，保留暂存副本和所有证据。归档名含时间戳，不覆盖先前相同审阅的记录。

同级保护用于原目录暂时不存在的窗口。Store打开前和取得写锁后、升级解除前/写锁内均检查；普通备份restore和迁移拒绝绕过未完成恢复。检查发现IPC endpoint会先创建目录，已补endpoint及connect_or_start入口检查，防止新版连接或启动在恢复窗口留下空目录。旧版程序不认识此同级保护，不能宣称约束所有旧客户端；若旧程序或外部进程创建竞争目标，当前恢复拒绝覆盖并保留证据。

7项专项通过：精确恢复与批准保持、过期审阅/活动写锁/错备份与hold拒绝、真实Windows目录改名共享冲突后保留意图并显式继续、目录空档保护/恢复、竞争目标与篡改暂存拒绝、放弃准备保留全部证据、损坏保护记录及操作互斥。目录空档为明确构造的中断状态，不是本次真实进程终止。最初核心变更workspace497通过/0失败/176忽略；随后IPC入口与归档名小改后重新通过7项恢复、IPC专项、resident_client集成、严格workspace全targets clippy和开发核心构建。没有用较早全量结果冒充最终每个文件都重跑全量。

实际开发核心CLI在三个独立夹具中通过：正常保留/恢复/finish/明确解锁，放弃准备，以及原目录已移走的受控中断状态续接。正常恢复中测试驱动没有代替产品搬移或删除目录，故障目录与备份恢复均逐文件摘要核对；冻结0.1.4重新读回原批准计划通过。第三例驱动明确构造移动意图/保留目录后，实际service start与status均拒绝且没有重建原目录，再由产品CLI继续恢复。首轮驱动在纯review后将Windows长路径前缀与普通路径作字符串比较，断言失败；未创建恢复事务。改为实际目录身份比较后使用新夹具通过，原失败记录保留。测试进程已退出，清理通过；无模型或用户桌面动作。

当前源码和debug核心含未发行的新行为，版本字段暂仍0.1.6；debug桌面/sidecar未同步，不能混用。原冻结0.1.6安装包及106归档131项摘要保持，未打包或发布同版本新二进制。后续发行必须统一升版、能力声明和维护工具后重新构建/冻结。尚缺图形入口、finish/abandon响应丢失后的归档查询、准备记录残缺的引导处置、与程序更新控制器的双向互斥，以及真实进程退出/磁盘满的完整事务矩阵；本轮只验核心/CLI限定路径，不宣称自动恢复产品整体已验收。

完整P3/P4/P5/P6未通过，目标active。下一步优先补上述事务收尾/维护工具整合和新包，再继续原剩余存储故障与安装验收。用户暂无签名证书/独立Windows环境的决定保持，不重复询问。安装版完整React入口、干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务及所有原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督及075/085观察仍保留。WorkBuddy064不重发，不关机、不关闭用户程序，无提交推送。

[107验收索引](../reports/p6-recovery-transaction-20261012.json)；[数据迁移说明](DATA-FORMAT-MIGRATION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第015项（106）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第016项 / 107' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第016项（107），progress，开发核心/CLI保留目录恢复事务通过，尚未发行，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方106为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第016项 / 107 保留目录恢复事务\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 107 开发核心/CLI恢复事务（限定范围通过，未发行）\n\n' + stamp + '\n\n' + body)
with (root / 'docs/DATA-FORMAT-MIGRATION-V1.md').open('a', encoding='utf8') as stream:
    stream.write('''\n\n## 107 开发版保留恢复事务（尚未发行）

开发核心新增 `recovery review/prepare/status/apply/finish/abandon`。review/prepare要求`--from`、`--manifest-digest`、`--hold-digest`，prepare再要求`--review-digest`；apply/finish/abandon要求同一审阅摘要。prepare只保存事务并准备副本；apply保留完整故障目录再恢复原位置；finish核对并归档保护，hold仍需明确release。status只读；失败先核对同一事务，不自动重发。abandon仅可撤销尚未有移动意图的准备，保留全部副本。

新版Store、IPC连接/启动及hold解除识别同级恢复保护；旧软件不认识它，竞争目标出现时恢复拒绝覆盖。图形入口和程序更新控制器尚未整合，finish/abandon结果丢失时的归档查询、残缺计划引导及完整进程故障矩阵待补，不用于当前用户数据。冻结0.1.6仍为先前工具，新CLI只存在开发核心；后续统一升版打包。证据见[107索引](../reports/p6-recovery-transaction-20261012.json)。\n''')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 107：开发核心/CLI保留恢复事务通过，冻结0.1.6发行包未变。\n\n- 下一步补归档完成查询、残缺准备处置、与程序更新工具双向互斥、图形入口及真实中断矩阵；统一升版后打包，不能混用新debug核心与旧桌面/sidecar。\n- 继续真实磁盘满/同步失败/断电、完整安装版入口、干净机器、签名等原门槛；用户暂无独立Windows及证书，不重复询问。\n- 完整P3/P4/P5/P6未通过，全部首批接入/管理/两桌面/编程效率/预算范围保持。\n- 先读[最新交接](HANDOFF-LATEST.md)与[107证据](../reports/p6-recovery-transaction-20261012.json)。\n\n以下106及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8')
start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 107）：开发核心/CLI已补完整目录保留与备份恢复事务，并通过限定验收；尚未整合图形维护或发行，冻结0.1.6包保持。先读[最新交接](docs/HANDOFF-LATEST.md)，按剩余门槛继续。完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['crates/core/src/backup_recovery.rs', 'crates/core/src/backup.rs', 'crates/core/src/lib.rs', 'crates/core/src/ipc.rs', 'crates/core/src/upgrade.rs', 'crates/core/src/workbench_cli.rs', 'scripts/check-recovery-transaction-20261012.py', 'scripts/finalize-recovery-transaction-20261012.py', 'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', 'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-recovery-transaction-*-20261012.log'))
paths.update(p for p in run.rglob('*') if p.is_file())
failed_run = reports / Path(failed['run']).name
paths.update(p for p in failed_run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '107', 'at': stamp, 'status': 'development-core-cli-reviewed-preservation-recovery-verified', 'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False, 'native': native, 'firstDriverPathComparisonFailure': failed, 'workspaceBeforeFinalIpcChange': workspace, 'finalFollowupTests': followups, 'finalRecoveryTestsPassed': 7, 'strictClippy': 'passed', 'developmentBuild': 'passed', 'productionSourceChanged': True, 'shippedPackageChanged': False, 'shippedRecoveryAccepted': False, 'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path), 'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path), 'previousArchiveRecordsVerified': len(previous['records']), 'remaining': ['finish/abandon lost-ACK history queries', 'incomplete preparation guidance', 'program updater interlock', 'maintenance window integration and versioned release package', 'real interruption and storage fault matrix', 'all original P3/P4/P5/P6 gates including full installed desktop, clean machine, signing/update service'], 'records': records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
