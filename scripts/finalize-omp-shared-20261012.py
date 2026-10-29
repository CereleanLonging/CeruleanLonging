"""Freeze 089 and update repository handoffs after authoritative local checks."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-omp-shared-20261012.json'
archive = reports / 'p5-omp-shared-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read(stage):
    raw = (reports / f'p5-omp-shared-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

previous = reports / 'p5-omp-coordinator-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
assert len(prior['records']) == 100
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name in ['crates/core/src/api_execution.rs', 'crates/core/src/workflow_coordinator.rs',
             'crates/core/src/coordinated_omp.rs', 'crates/core/src/omp_workflow_access.rs',
             'crates/core/src/git_workspaces.rs', 'crates/core/src/verifier.rs',
             'crates/core/src/verifier/process.rs', 'crates/core/src/omp_process.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
for name in prior['inheritedUiSources']:
    record = next(r for r in ui['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
full = read('full')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 457, counts
assert sum(int(i) for _, i in counts) == 167, counts
assert '301 passed; 0 failed; 148 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
checks = {'parallel': 1, 'ack2': 1, 'worktree': 4, 'dag': 1, 'api-registry': 2, 'mixed': 2, 'crash': 1}
for stage, count in checks.items():
    assert f'{count} passed; 0 failed' in read(stage), stage
for first in ['true', 'false']:
    for cancel in ['true', 'false']:
        assert f'omp_first={first} cancel={cancel}' in read('parallel')
for window in ['prepare', 'commit', 'lost-commit']:
    assert f'OMP shared ACK {window}:' in read('ack2')
for stage in ['clippy', 'desktop']:
    assert 'Finished `dev`' in read(stage), stage
crash_path = reports / 'p5-omp-shared-crash-result-20261012.json'
crash = json.loads(crash_path.read_text(encoding='utf-8-sig'))
assert crash['status'] == 'passed' and crash['realModelCalls'] == 0 and len(crash['cases']) == 5
assert all(c['status'] == 'passed' and c['actualOwnerKill'] and c['workerRestarts'] == 0 for c in crash['cases'])
cases = reports / 'p5-omp-shared-cases-20261012'
results = []
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    result = json.loads((cases / mode / 'result.json').read_text(encoding='utf-8-sig'))
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['finalReopenNewEvents'] == 0
    if mode != 'failure-after-cli':
        assert result['accepted'] == 3 and result['dispatchesPerTask'] == 1 and result['intermediateRestart'] and result['managementRequests'] == 7
        for name, expected in [('a.txt', 'A'), ('b.txt', 'B'), ('bundle.txt', 'AB')]:
            assert (Path(result['finalDirectory']) / name).read_text(encoding='utf-8') == expected
    else:
        assert result['accepted'] == 1 and result['finalOmpDispatches'] == 0
        assert result['resumeBlocked'] and result['partialWritePreserved'] and result['apiState'] == 'unknown'
    results.append(result)
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
peers = {name: checksum(root / name) for name in ['target/debug/examples/omp_offline_worker_fixture.exe', 'target/debug/examples/codex_dag_fixture.exe']}
seconds = {stage: float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', read(stage))[-1]) for stage in checks}
stamp = datetime.now().astimezone().isoformat()
body = '''089开放批准的独立worktree任务在同项目内OMP/API自动并行。OMP活动与取消改按(project,attempt)定位，原api_writers登记表现在共同持有API、OMP和独立验收的唯一账本。执行器Done仅返回结果及中断标志；只有最后一个使用者退出才归还原DurableWorkflow。项目投影仅来自协调器，迟到的OMP Progress不能覆盖或重复项目；OMP进程回收未确认则保留Unknown活动与所有者。原预算、模型槽2、后台操作上限4、工具/进程隔离及执行器权限保持。

OMP并行启动、两次管理确认及重开复用原不可变历史前缀校验：新受管独立目录启动使用StartupIntent schemaVersion=2，锚定原副指挥上下文。目标任务、方案/批准、依赖、配置和用户答复必须保持，允许其他任务的限定生命周期事件交错。Prepare核对原历史快照，Commit核对原管理目标、实际磁盘事件及对应快照；没有重建批准或补发管理/模型请求。旧version=1仍读取原记录，未改写；新版本记录会被旧程序拒绝，不能据此宣称P6回滚兼容。version=2仅允许受管理的OMP启动，Codex旧协议不扩权。

自动运行沿原候选、资源和额度关口选择OMP或API。OMP等待原Prepare/Commit时不会另建分配；API/OMP执行结束后可在另一个仍运行时进行独立验收与主指挥Accept，验收必须等待对应执行线程退出。取消只触达原attempt，同时按原用户语义暂停项目新派发；其他已发送任务继续收集结果。Unknown、失败及未决启动仍持有原资源。

新增实际Git/OMP子进程/API线程检查：四种自动场景覆盖OMP先启动和API先启动、两种正常交错收口及两种单独取消OMP。正常各3任务独立验证后Accepted、7次管理请求、最终AB；取消保留另一API的Submitted和OMP Cancelled，最终集成零派发。错误attempt停止被拒绝，旧Progress不会倒退，始终一个所有者，重开无新增事件。另三种ACK窗口覆盖Prepare前或Commit前API完成，以及Commit已保存但回执丢失；原上下文仍有效、伪造快照拒绝，丢失确认时OMP零模型派发、原Assigned保留且不重发。

原三执行者串行/中途重开对照夹具现在明确批准共同资源互斥，保留原对照意图；新增并行夹具不声明这项互斥。不能继续依赖旧实现偶然串行来限制测试。实际owner终止五个管理窗口沿用原version=1夹具，零worker重启；新版交错覆盖来自新增version=2的三场景，不把旧终止检查扩大称为新版全部崩溃矩阵。

初轮编译发现共享验收借用范围和旧夹具按项目索引/归还账本接口未同步，均已改为新生命周期。ACK夹具首次回复ID含非法连字符，被原校验拒绝；改用原允许格式，生产校验未放宽。所有失败日志保留。没有重跑原桌面基础接入或WorkBuddy064已消费的sum任务。
'''
body += (
    f'\n最终检查：457项默认Rust/167项显式忽略（核心301/148）、5项Node、前端与核心构建通过；'
    f'新自动并行1项4场景{seconds["parallel"]}秒、ACK专项1项3场景{seconds["ack2"]}秒、'
    f'OMP工作目录4项{seconds["worktree"]}秒、OMP依赖1项4组合{seconds["dag"]}秒、'
    f'API登记2项{seconds["api-registry"]}秒、混合宿主2项{seconds["mixed"]}秒、'
    f'原owner终止1项5窗口{seconds["crash"]}秒通过。严格workspace全targets clippy与桌面custom-protocol debug构建通过。'
    f'三个runtime摘要一致：{next(iter(runtimes.values()))}。\n\n'
    '原088的100份冻结归档与079的6份界面源文件摘要保持；原协调器命令/预算事务、API生产派发、Git与独立验证器及OMP进程隔离源码未改。'
    '模型和主管回复受控，真实模型调用/用户桌面动作0。完整P4/P5/P6仍未验收，目标active。'
    '下一步迁移Codex原生轮次、监督及取消到同一所有者，补完整异构生命周期和故障矩阵，再验收真实OpenAI管理、全部首批接入、真实编程/含失败的效率及P6安装/升级/回滚/迁移/签名/支持矩阵。'
    '075 Job退出观察、085投影共享冲突占用方未定结论保留；OMP金额硬预算和运行中监督仍未开放。\n'
)

names = {r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
names.update([
    'AGENTS.md', 'crates/core/examples/omp_offline_worker_fixture.rs',
    'crates/core/src/omp_shared_host.rs', 'crates/core/src/omp_management_bridge.rs',
    'crates/core/src/omp_management_bridge/tests.rs', 'crates/core/src/omp_management_bridge/assignment_crash_tests.rs',
    'crates/core/src/omp_host/tests/worktree.rs', 'crates/core/src/omp_restore_host.rs',
    'crates/core/src/integration_tests/omp_parallel.rs', 'crates/core/src/shared_verification_host.rs',
    'scripts/check.ps1', 'scripts/check-omp-shared-20261012.ps1', 'scripts/finalize-omp-shared-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/NEXT-STAGE-GATES.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
])
paths = {root / name for name in names}
paths.update(reports.glob('p5-omp-shared-*-20261012.log'))
paths.add(crash_path)
for mode in ['final-api', 'final-codex', 'final-omp', 'failure-after-cli']:
    paths.update(cases / mode / name for name in ['result.json', 'snapshot.json', 'bin/protocol.jsonl'])
assert all(p.is_file() for p in paths), [str(p) for p in paths if not p.is_file()]

latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf-8-sig')
marker = '当前：2026-10-12 卷001第017项（088）'
assert marker in old and '089最终检查已通过' not in old
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷001第018项（089），progress，089最终检查已通过。上一轮088为progress。\n\n'
    + stamp + '\n\n' + body + '\n[089验收索引](../reports/p5-omp-shared-20261012.json)。\n\n下方088为冻结历史。\n\n' + old[old.index(marker):], encoding='utf-8')
for name in ['docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md',
             'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md']:
    p = root / name
    text = p.read_text(encoding='utf-8-sig')
    assert '089 OMP与API共享调度' not in text
    text = text.replace('更新至批次088', '更新至批次089').replace('本页088小节', '本页089小节')
    heading = '第018项 / ' if name.startswith('handoffs/') else ''
    prefix = '../../' if name.startswith('handoffs/') else '../'
    p.write_text(text + '\n\n## ' + heading + '089 OMP与API共享调度（本批检查通过，完整目标active）\n\n' + stamp + '\n\n' + body
        + '\n[089验收索引](' + prefix + 'reports/p5-omp-shared-20261012.json)。\n', encoding='utf-8')
p = root / 'docs/NEXT-STAGE-GATES.md'
text = p.read_text(encoding='utf-8-sig')
tail = text[text.index('以下为较早阶段的历史记录。'):]
lead = '''# 后续阶段的验收执行顺序

2026-10-12 / 089 更新：OMP/API在批准的独立worktree下已开放同项目自动并行，原单写账本、两次管理确认、按尝试取消、共享独立验收与主指挥收口均通过相应受控检查。先读[最新交接](HANDOFF-LATEST.md)、[当日矩阵](STAGE-ACCEPTANCE-2026-10-12.md)和[P5记录](P5-WORKTREE-IMPLEMENTATION.md)。完整P4/P5/P6尚未通过。

- 下一步将Codex的DurableWorkflow独占执行迁到已有共享持有者，按(project,attempt)管理活动；原native turn、工具/产物锚点、建议送达、取消/双ACK和恢复必须逐项保留，不能只删独占保护。复用088/089及旧Codex验证，补差异。
- Codex迁移的具体耦合点：WorkerChannel::wait_supervised仍接收DurableWorkflow，supervised_worker::advice会发布原建议命令，started_cancellation_anchors须读取原已开始轮次。保留原公开派发/恢复契约，抽出受限执行能力；原Codex请求计数仍按DispatchIntent个数，接共享前须复用已有API全轮数/CLI1次的原子预算关口。
- API/OMP/独立验收共用现有api_writers登记表，最后使用者退出才归还账本；原执行线程尚在收尾时不能验收其产物。API原子轮数预留、CLI1次及Unknown保留规则不变。模型槽2、后台操作4、项目验收1保持。
- OMP新的受管独立目录StartupIntent为schemaVersion=2，旧version=1保持。新版前缀校验支持其他任务合法事件交错；旧版本拒绝新记录。后续P6必须把此兼容边界纳入迁移及回滚策略，不能用新程序读取旧数据代替旧程序读取新数据。
- 补全异构故障矩阵：新并行三ACK场景与旧version=1五次实际owner终止分别有证据，新版共享情况下的完整崩溃/负载和OMP监督/金额预算支持仍不可夸大。075 Job退出与085投影共享冲突观察继续保留未定根因。
- 全部首批接入、真实OpenAI主副指挥、桌面产品可靠性和真实异构编程/含失败的效率保持范围。优先引用旧真实接入；WorkBuddy064单次授权已用完，不重发原sum任务。
- P6须完成干净目标安装/卸载、升级与失败恢复、备份/数据迁移、回滚的不兼容拒绝、签名和支持矩阵；debug构建、受控模型结果及选择性归档不能替代。

'''
p.write_text(lead + tail, encoding='utf-8')

archive.mkdir()
records = []
for path in sorted(paths):
    relative = path.relative_to(root)
    destination = archive / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, destination)
    value = checksum(path)
    assert checksum(destination) == value
    records.append({'source': relative.as_posix(), 'snapshot': destination.relative_to(root).as_posix(), 'sha256': value})
report = {
    'batch': '089', 'at': stamp, 'status': 'approved-isolated-omp-api-automatic-parallel-enabled', 'goalStatus': 'active',
    'fullP4Accepted': False, 'fullP5Accepted': False, 'fullP6Accepted': False,
    'rustDefaultPassed': 457, 'rustDefaultIgnored': 167, 'defaultCorePassed': 301, 'defaultCoreIgnored': 148, 'nodeTests': 5,
    'sameProjectOmpApiParallelEnabled': True, 'sameProjectCodexParallelEnabled': False,
    'automaticOmpApiParallelEnabled': True, 'executionWorkerLimitGlobal': 2, 'backgroundOperationLimitGlobal': 4,
    'parallelScenarios': 4, 'ackWindowScenarios': 3, 'legacyActualOwnerKills': 5,
    'newOmpStartupSchema': 2, 'legacyOmpStartupSchemaPreserved': True, 'oldBinaryReadsNewSchema': False,
    'runtimeHashes': runtimes, 'offlinePeerHashes': peers, 'inheritedUiSources': prior['inheritedUiSources'],
    'realModelCalls': 0, 'userDesktopActions': 0, 'mixedHostResults': results,
    'executionScope': 'real Git/OMP processes/API threads/tools/verifier; controlled chief and provider replies',
    'newJournal': False, 'executionWorkerAcceptanceAuthority': False,
    'failuresPreserved': ['initial borrow and fixture ownership/index compilation errors', 'ACK fixture response ID had a hyphen and original validation rejected it; fixture corrected, production validation retained'],
    'inheritedSpecializedChecks': 'initial four automatic scenarios and final three ACK windows reused; only later new default startup compatibility check and fixture response format changed; full defaults compile final sources',
    'unresolvedObservation': prior['unresolvedObservation'], 'projectionSharingObservation': prior['projectionSharingObservation'],
    'remaining': ['Codex shared native-turn/supervision/cancel/startup ownership and full heterogeneous scheduling',
        'new parallel crash/load matrix, real coding and efficiency including failures',
        'all first-batch access, real OpenAI management, desktop reliability, P6 install/update/rollback/migration/signing/support'],
    'specializedCheckSeconds': seconds,
    'previousIndex': previous.relative_to(root).as_posix(), 'previousIndexSha256': checksum(previous),
    'archiveScope': 'selective source and local evidence, not a full reproducible repository', 'records': records,
}
with index.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2)
    stream.write('\n')
print(json.dumps({'index': str(index), 'records': len(records), 'rustPassed': 457, 'rustIgnored': 167}, ensure_ascii=False))
