"""Archive 108: bounded, read-only recovery evidence queries after ACK loss."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil

root = Path(r'\\?\D:\test\CeruleanLonging'); reports = root / 'reports'
index = reports / 'p6-recovery-inspect-20261012.json'
archive = reports / 'p6-recovery-inspect-sources-20261012'
assert not index.exists() and not archive.exists()
def digest(path): return sha256(path.read_bytes()).hexdigest()
def load(path): return json.loads(path.read_text(encoding='utf-8-sig'))
def log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous_path = reports / 'p6-recovery-transaction-20261012.json'; previous = load(previous_path)
unchanged = ['crates/core/src/backup.rs', 'crates/core/src/lib.rs', 'crates/core/src/ipc.rs', 'crates/core/src/upgrade.rs']
for item in previous['records']:
    assert digest(root / item['snapshot']) == item['sha256']
    if item['source'] in unchanged:
        assert digest(root / item['source']) == item['sha256']
package_path = root / previous['packageManifest']
assert digest(package_path) == previous['packageManifestSha256']
for item in load(package_path)['artifacts']:
    assert digest(package_path.parent / item['path']) == item['sha256']
native = json.loads(log(reports / 'p6-recovery-inspect-native1-20261012.log'))
run = reports / Path(native['run']).name
assert load(run / 'result.json') == native
assert native['status'] == 'passed' and native['cleanup'] == 'passed' and not native['pending']
assert [x['action'] for x in native['cases']] == ['finish', 'abandon']
assert native['realChildTermination'] and not native['instrumentedProduct'] and not native['shippedReleaseAccepted']
assert digest(root / 'target/debug/cerulean-core.exe') == native['developmentCoreSha256']
for case in native['cases']:
    assert case['hasExited'] and case['exitCode'] != 0 and case['ackBytes'] == 0
    assert case['inspectionReadOnly'] and case['verifiedBeforeAndAfterKill'] and case['singleFinalizationAttempt']
    evidence = run / case['action']
    assert (evidence / 'worker.stdout').stat().st_size == 0
    before = load(evidence / 'observed-before-kill.json')
    assert before['pid'] == case['pid'] and before['alive']
    assert before['inspection']['archives'][0]['verified'] and not before['inspection']['guardPresent']
assert '10 passed; 0 failed' in log(reports / 'p6-recovery-inspect-tests1-20261012.log')
assert '1 passed; 0 failed' in log(reports / 'p6-recovery-inspect-cli-20261012.log')
for name in ['clippy', 'build']:
    assert 'Finished `dev` profile' in log(reports / f'p6-recovery-inspect-{name}-20261012.log')
assert (reports / 'p6-recovery-inspect-fmt-20261012.log').exists()
stamp = datetime.now().astimezone().isoformat()
body = '''108 补齐开发核心恢复事务finish/abandon返回丢失后的只读核查。新增recovery inspect --review-digest，同时报告当前同级保护/活动事务与每条匹配归档；没有匹配证据不等于未执行，旧归档成功也不代表同审阅的新事务完成。查询没有恢复、解锁、启动或重试动作，明确currentDataVerified=false、automaticReplay=false。

收尾前新增持久finalization回执，绑定审阅、动作、时间、当时原位置文件清单摘要、事务JSON记录和保留/暂存文件清单。归档名包含回执摘要，禁止覆盖；若实际改名失败，显式再次finish复用同一回执及目的地，拒绝变化的原意图。发布后再次核对归档，原hold仍保留。查询逐项核对原路径/SID/审阅、原备份、回执和全部保留/暂存文件；每条损坏或不支持的归档单独标为未核验，不伪称成功。只核验归档证据，不声称当前工作数据仍与当时相同。当前实现要求原备份仍可核验，父目录最多扫描4096项，记录读取/写入上限16MiB。

恢复专项10项通过，包含原107七项、完成后只读历史核查及保留文件篡改、相同审阅反复准备/放弃并区分当前未决事务、真实共享冲突导致归档失败后复用原收尾意图。CLI参数检查1项、格式检查、严格workspace全targets clippy及开发核心构建通过。107的备份/核心启动/IPC/解除实现摘要未变，继承原497项workspace基线和随后IPC/resident专项，不重跑基础接入或用窄检查宣称整个P6通过。

实际开发核心进行了两次无插桩的返回丢失实验。父进程预填4096字节输出管道，finish或abandon只执行一次；观察原保护目录已归档、只读inspect已核验且原进程仍存活后，通过原Popen句柄终止该测试进程并等待退出。finish PID24392、abandon PID13168均实际退出1，收到的业务返回字节均为0。重新inspect结果与终止前相同，查询前后原数据及归档文件摘要不变，没有重发finish/abandon。完成恢复的一例明确解除hold后，冻结0.1.4读回原批准计划；再查历史仍可核验，但不宣称当前数据未变化。放弃一例原故障数据与hold保持。两例清理通过，无模型或用户应用动作。

107冻结归档217项与原0.1.6安装包全部摘要保持。当前源码/debug核心仍是未发行开发组合，版本字段暂0.1.6，旧debug桌面/sidecar未同步；未重打同版本发行包。107旧开发归档没有新finalization回执，会明确显示未核验，不补造历史记录。本次真实子进程退出不是物理断电、磁盘满、GUI或已发行包验收。

完整P3/P4/P5/P6仍未通过，目标active。下一步补残缺准备的引导处置、与程序更新控制器双向互斥、图形维护入口及统一升版构建，再补完整事务中断/存储故障和安装验收。用户暂无签名证书及独立Windows环境，继续其他验收，不重复询问。原安装版完整React入口/干净机器、统一程序数据事务、跨用户路径机器、签名/更新服务及全部首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/含失败效率、共享故障负载、金额预算/按执行者监督和075/085观察保持。WorkBuddy064不重发，不关机、不关闭用户程序，无提交推送。

[108验收索引](../reports/p6-recovery-inspect-20261012.json)；[数据迁移说明](DATA-FORMAT-MIGRATION-V1.md)。
'''
latest = root / 'docs/HANDOFF-LATEST.md'; old = latest.read_text(encoding='utf8')
assert '卷002第016项（107）' in old.splitlines()[2]
volume = root / 'handoffs/2026-10-12/handoff_2026-10-12_002_150524+0800.md'
assert '第017项 / 108' not in volume.read_text(encoding='utf8')
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第017项（108），progress，恢复收尾只读核查与两次实际返回丢失通过，尚未发行，完整P6未通过。\n\n' + stamp + '\n\n' + body + '\n下方107为冻结历史。\n\n' + old.removeprefix('# 最新交接\n\n'), encoding='utf8')
with volume.open('a', encoding='utf8') as stream:
    stream.write('\n\n## 第017项 / 108 恢复收尾查询与返回丢失\n\n' + stamp + '\n\n' + body.replace('(../reports/', '(../../reports/').replace('(DATA-FORMAT-MIGRATION-V1.md)', '(../../docs/DATA-FORMAT-MIGRATION-V1.md)'))
with (root / 'docs/STAGE-ACCEPTANCE-2026-10-12.md').open('a', encoding='utf8') as stream:
    stream.write('\n\n## 108 恢复收尾核查（开发核心限定范围通过）\n\n' + stamp + '\n\n' + body)
with (root / 'docs/DATA-FORMAT-MIGRATION-V1.md').open('a', encoding='utf8') as stream:
    stream.write('''\n\n108新增 `recovery inspect --review-digest <原审阅摘要>`：分别列出活动事务和匹配的归档核验结果，旧成功不消除当前未决事务，不自动重试或解锁。finish/abandon在归档前保存finalization回执，归档名绑定其摘要；查询核对备份、回执、保留/暂存文件，损坏或旧无回执归档标为未核验。它证明历史归档证据，不证明当前数据仍是旧快照。原备份必须仍可核验，父目录扫描上限4096项、记录上限16MiB。实际开发核心两次归档后返回丢失均通过只读查询确认；尚未发行或接入图形维护。详见[108证据](../reports/p6-recovery-inspect-20261012.json)。\n''')
gates = root / 'docs/NEXT-STAGE-GATES.md'
gates.write_text('# 后续阶段的验收执行顺序\n\n2026-10-12 / 108：恢复收尾归档查询及finish/abandon真实返回丢失通过，仍为未发行开发核心。\n\n- 下一步补残缺准备引导、程序更新控制器双向互斥、维护窗口及统一升版构建；原收尾查询缺口已由本项补齐，不重做基础接入。\n- 继续完整事务中断/磁盘满/同步失败/断电及原P3/P4/P5/P6全部门槛。独立Windows和签名证书暂无，按用户指示继续其他验收。\n- 冻结0.1.6包保持，不能混用新debug核心与旧桌面/sidecar。\n- 先读[最新交接](HANDOFF-LATEST.md)与[108证据](../reports/p6-recovery-inspect-20261012.json)。\n\n以下107及更早为历史。\n\n' + gates.read_text(encoding='utf8').removeprefix('# 后续阶段的验收执行顺序\n\n'), encoding='utf8')
readme = root / 'README.md'; text = readme.read_text(encoding='utf8')
start = text.index('最新进展（'); end = text.index('\n\n', start)
readme.write_text(text[:start] + '最新进展（2026-10-12 / 108）：开发核心恢复收尾归档查询通过，finish/abandon实际返回丢失后均可只读核验，未重发。尚未整合图形维护或发行，冻结0.1.6包保持。先读[最新交接](docs/HANDOFF-LATEST.md)，完整P0–P6尚未通过。' + text[end:], encoding='utf8')
names = ['crates/core/src/backup_recovery.rs', 'crates/core/src/workbench_cli.rs', 'scripts/check-recovery-finish-ack-20261012.py', 'scripts/finalize-recovery-inspect-20261012.py', 'README.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', 'docs/DATA-FORMAT-MIGRATION-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
paths = {root / name for name in names} | {volume, package_path}
paths.update(reports.glob('p6-recovery-inspect-*-20261012.log'))
paths.update(p for p in run.rglob('*') if p.is_file())
archive.mkdir(); records = []
for path in sorted(paths):
    relative = path.relative_to(root); snapshot = archive / relative
    snapshot.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, snapshot)
    value = digest(path); assert digest(snapshot) == value
    records.append({'source': relative.as_posix(), 'snapshot': snapshot.relative_to(root).as_posix(), 'sha256': value})
report = {'batch': '108', 'at': stamp, 'status': 'recovery-finalization-inspection-and-real-lost-ack-verified', 'goalStatus': 'active', 'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False, 'native': native, 'recoveryTestsPassed': 10, 'cliTestsPassed': 1, 'strictClippy': 'passed', 'developmentBuild': 'passed', 'shippedRecoveryAccepted': False, 'shippedPackageChanged': False, 'packageManifest': package_path.relative_to(root).as_posix(), 'packageManifestSha256': digest(package_path), 'previousIndex': previous_path.relative_to(root).as_posix(), 'previousIndexSha256': digest(previous_path), 'previousArchiveRecordsVerified': len(previous['records']), 'inheritedUnchangedSources': unchanged, 'remaining': ['incomplete preparation guidance', 'program updater interlock', 'maintenance window integration/versioned release', 'full real interruption/storage-fault matrix', 'all original P3/P4/P5/P6 including installed React, clean machine, signing, original providers/desktops/real management/coding/efficiency'], 'records': records}
with index.open('x', encoding='utf8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'goalStatus': 'active'}))
