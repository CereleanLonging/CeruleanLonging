"""Freeze batch 090 only after all original and scoped regression gates pass."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-codex-coordinator-20261012.json'
archive = reports / 'p5-codex-coordinator-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def checksum(path):
    return sha256(path.read_bytes()).hexdigest()
def read(stage):
    raw = (reports / f'p5-codex-coordinator-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous = reports / 'p5-omp-shared-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
for name in ['crates/core/src/api_execution.rs', 'crates/core/src/coordinated_omp.rs',
             'crates/core/src/omp_workflow_access.rs', 'crates/core/src/git_workspaces.rs',
             'crates/core/src/verifier.rs', 'crates/core/src/verifier/process.rs',
             'crates/core/src/omp_process.rs', 'crates/core/src/omp_shared_host.rs',
             'crates/core/src/omp_management_bridge.rs', 'crates/core/src/startup_journal.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
for name in prior['inheritedUiSources']:
    record = next(r for r in ui['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
full = read('full')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
assert sum(int(p) for p, _ in counts) == 460, counts
assert sum(int(i) for _, i in counts) == 168, counts
assert '304 passed; 0 failed; 149 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
checks = {'budget2': 9, 'worktree2': 4, 'dag-success': 1, 'dag-recover-a': 1, 'crash': 1}
for mode in ['ACK', 'WRONG_ACK', 'LOST_ACK', 'PAUSE', 'PAUSE_COMMIT', 'CANCEL']:
    checks['advice-' + mode] = 1
for stage, count in checks.items():
    assert f'{count} passed; 0 failed' in read(stage), stage
for stage in ['fixtures', 'clippy', 'desktop']:
    assert 'Finished `dev`' in read(stage), stage
for scope in ['completed', 'cancel-empty', 'cancel-lost', 'wrong-proof', 'missing']:
    assert f'Codex scoped {scope}:' in read('scoped1')
for race, budget in [('true', 1), ('false', 3), ('false', 4)]:
    assert f'Codex/API budget race={race} budget={budget}:' in read('budget2')
case_paths = []
case_summaries = []
for mode in ['success', 'recover-a']:
    base = reports / f'p5-codex-coordinator-dag-{mode}-20261012'
    result = json.loads((base / 'result.json').read_text(encoding='utf-8'))
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['finalRestartNewEvents'] == 0
    assert result['turnStarts'] == 2 and result['bDispatchBeforeAAccept'] == 0 and result['checksPerTask'] == 2
    case_summaries.append(result)
    case_paths.extend(p for p in base.glob('*.json') if p.is_file())
    case_paths.append(base / 'bin/protocol.jsonl')
for mode in ['ACK', 'WRONG_ACK', 'LOST_ACK', 'PAUSE', 'PAUSE_COMMIT', 'CANCEL']:
    base = reports / f'p5-codex-coordinator-advice-{mode}-20261012'
    result = json.loads((base / 'delivery-result.json').read_text(encoding='utf-8'))
    assert result['status'] == 'passed' and result['realModelCalls'] == 0 and result['newEventsAfterRestart'] == 0
    case_summaries.append({k: v for k, v in result.items() if k not in ['snapshot', 'details']})
    case_paths.extend(p for p in base.glob('*.json') if p.is_file())
    case_paths.append(base / 'bin/protocol.jsonl')
base = reports / 'p5-codex-coordinator-crash-cases-20261012'
crash = json.loads((base / 'crash-result.json').read_text(encoding='utf-8'))
assert crash['status'] == 'passed' and crash['realModelCalls'] == 0 and len(crash['cases']) == 5
assert all(c['realTermination'] and c['peerExited'] and c['realModelCalls'] == 0 for c in crash['cases'])
case_paths.append(base / 'crash-result.json')
for point in ['prepared', 'advised', 'intent', 'received', 'acknowledged']:
    case_paths.extend(base / point / name for name in ['snapshot.json', 'bin/protocol.jsonl', 'owner.stdout', 'owner.stderr'])
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1
seconds = {stage: float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', read(stage))[-1]) for stage in checks}
stamp = datetime.now().astimezone().isoformat()
body = '''090将Codex生产执行内部迁到既有单写协调器。后台线程仅持有绑定项目、批准修订、任务、attempt及Codex适配器的客户端；原账本在协调器线程持有。原生等待、Git准备、主副指挥双确认和建议送达均不占写线程。线程退出先释放客户端，再归还原DurableWorkflow；客户端确认丢失、写入中断或执行panic必须按原账本重开，不能把本地缓存当成已确认结果。

原公开dispatch/dispatch_controlled/reconcile和WorkerChannel::wait_supervised入口保持。新增opaque WorkerWorkflow及默认wait_scoped方法供内部协调路径使用，不对外暴露所有者。原operation ID、thread/turn、Started/CancelIntent双锚点、产物与恢复证据保持。取消锚点由写线程执行原校验。副指挥建议沿原Prepare/Commit、管理事件和delivery journal/gate发布，新增通道还绑定原运行attempt及mgmt操作；未授予方案批准、Verify或Accept能力，API/OMP原命令通道未扩权。

Codex预检和派发改复用原API请求计数：未封存API保留全部轮数，CLI派发计1，Unknown不退款；最终判断与原DispatchIntent在同一写线程内完成。真实Git三场景验证过期快照争用最后额度只发布一次，API预留3轮后Codex需要额外1次。没有引入第二预算或账本。

新增3项专项覆盖5种执行结果、权限拒绝和两种入账故障。模型等待时协调器能响应，仍有客户端不能转移账本；正常提交、空输入取消、取消ACK丢失后只读恢复、错误回读与缺失产物均保留原身份。失去派发入账确认或发布后中断时模型发送0，原意图保留，重开Unknown，不重发。首次编译发现Box<DurableWorkflow>的泛型调用缺少显式解引用，已修正且保留失败日志。专项脚本首次预算筛选零匹配、工作目录夹具缺环境变量，不能计作通过；修正筛选及原夹具路径后续跑，并加入零测试拒绝，已通过全量检查未重复。

复用原四项实际Codex工作目录检查、成功/只读恢复两种依赖DAG、六种建议送达/暂停/取消组合和五个实际owner终止窗口。模型和主管回复仍受控，真实Git、原Codex协议子进程、独立验收与账本恢复按生产路径运行；未重跑原桌面基础接入或已消费的WorkBuddy064单次sum任务。

本批只完成Codex执行内部所有权迁移：RuntimeHost仍按项目独占Codex，Done仍归还原DurableWorkflow。API/OMP自动并行继续沿089；本批不声称Codex与它们公开并行。下一步提升Codex协调器到原api_writers登记表，按(project,attempt)管理活动、取消、监督和重开，并迁移原启动前缀/双ACK；最后一个使用者退出才归还，不能只删除独占限制。
'''
body += (f'\n最终检查：460项默认Rust/168项显式忽略（核心304/149）、5项Node、前端/核心构建通过；'
    f'预算9项{seconds["budget2"]}秒、Codex工作目录4项{seconds["worktree2"]}秒、'
    f'两种依赖DAG分别{seconds["dag-success"]}/{seconds["dag-recover-a"]}秒、六种纠偏专项及五窗口实际终止通过。'
    f'严格workspace全targets clippy和桌面custom-protocol debug构建通过；三个runtime摘要一致：{next(iter(runtimes.values()))}。\n\n'
    f'原089的{len(prior["records"])}份冻结归档及079的6份界面源文件摘要保持。真实模型调用/用户桌面动作0，完整P4/P5/P6仍未验收，目标active。'
    '全首批接入、真实OpenAI管理、完整异构生命周期/故障/负载和真实编程效率（含失败），以及P6安装/升级/回滚/迁移/签名/支持矩阵仍在范围内。'
    '075 Job退出观察及085投影共享冲突占用方未定结论保留；OMP监督/金额预算及旧程序拒绝OMP启动schema2的兼容边界不变。\n')
names = {r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
names.update(['AGENTS.md', 'crates/core/src/async_codex.rs', 'crates/core/src/codex_execution.rs',
    'crates/core/src/codex_workflow_access.rs', 'crates/core/src/coordinated_codex.rs',
    'crates/core/src/coordinated_codex_tests.rs', 'crates/core/src/supervised_worker.rs',
    'crates/core/src/early_cancellation.rs', 'crates/core/src/advice_crash_tests.rs',
    'crates/core/src/project_dag_tests.rs', 'crates/core/src/codex_worktree_tests.rs',
    'crates/core/src/supervision_delivery.rs', 'crates/core/src/advice_delivery.rs',
    'crates/core/src/codex_management_bridge.rs', 'crates/core/tests/codex_cancellation.rs',
    'scripts/check.ps1', 'scripts/check-codex-coordinator-20261012.ps1',
    'scripts/migrate-codex-coordinator-20261012.py', 'scripts/finalize-codex-coordinator-20261012.py',
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/NEXT-STAGE-GATES.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md', 'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md'])
paths = {root / name for name in names} | set(case_paths) | set(reports.glob('p5-codex-coordinator-*-20261012.log'))
assert all(p.is_file() for p in paths), [str(p) for p in paths if not p.is_file()]
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf-8-sig')
marker = '当前：2026-10-12 卷001第018项（089）'
assert marker in old and '090最终检查已通过' not in old
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷001第019项（090），progress，090最终检查已通过。上一轮089为progress。\n\n'
    + stamp + '\n\n' + body + '\n[090验收索引](../reports/p5-codex-coordinator-20261012.json)。\n\n下方089为冻结历史。\n\n' + old[old.index(marker):], encoding='utf-8')
for name in ['docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md',
             'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md']:
    p = root / name
    text = p.read_text(encoding='utf-8-sig')
    assert '090 Codex执行协调器' not in text
    text = text.replace('更新至批次089', '更新至批次090').replace('本页089小节', '本页090小节')
    heading = '第019项 / ' if name.startswith('handoffs/') else ''
    prefix = '../../' if name.startswith('handoffs/') else '../'
    p.write_text(text + '\n\n## ' + heading + '090 Codex执行协调器（本批检查通过，完整目标active）\n\n' + stamp + '\n\n' + body
        + '\n[090验收索引](' + prefix + 'reports/p5-codex-coordinator-20261012.json)。\n', encoding='utf-8')
p = root / 'docs/NEXT-STAGE-GATES.md'
text = p.read_text(encoding='utf-8-sig')
tail = text[text.index('以下为较早阶段的历史记录。'):]
p.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 090 更新：Codex执行内部已迁到原单写协调器，公开派发/恢复契约、原生轮次取消和副指挥建议保留并通过差异检查。API/OMP公开自动并行继续沿089，Codex宿主仍独占。先读[最新交接](HANDOFF-LATEST.md)、[当日矩阵](STAGE-ACCEPTANCE-2026-10-12.md)和[P5记录](P5-WORKTREE-IMPLEMENTATION.md)。完整P4/P5/P6尚未通过。

- 下一步把Codex协调器提升到原api_writers共享登记表；活动按(project,attempt)索引，Done返回结果/中断/进程清理状态，最后使用者退出才归还。项目投影只取所有者，迟到Progress不可覆盖其他任务；不能仅删除独占准入。
- 原CodexStartupIntent仍version1。共享前须迁移原不可变前缀、Prepare/Commit双ACK与恢复交错，同时监督Prepare/Commit和建议送达绑定原attempt/turn，取消只触达该尝试并暂停后续调度。复用089 ACK机制及090原生证据，不重做基础接入。
- Codex请求预算已统一为API全轮数/CLI1次并在写线程原子发布；Unknown保留原额度。模型槽2、后台操作4、独立验收/资源关口保持。工作线程不能批准、Verify或Accept。
- 已通过Codex内部五种执行结果、权限/入账故障、四工作目录、两依赖DAG、六纠偏组合、五实际owner终止窗口。完整异构共享崩溃/负载尚缺，不扩大这些原项目独占证据。075 Job退出观察、085投影共享冲突未定根因继续保留。
- OMP schema2只适用于原受管理独立目录启动；旧version1保持，旧程序拒绝新记录。OMP运行中监督和金额硬预算仍未开放。P6须明确兼容迁移及回滚拒绝规则。
- 全部首批接入、真实OpenAI主副指挥、桌面可靠性、真实异构编程及包含失败的效率保持范围。引用已有真实接入；WorkBuddy064单次授权已消费，不重发原sum任务。
- P6仍须干净目标安装/卸载、升级/失败恢复、备份/迁移、回滚、签名和支持矩阵；debug构建及受控模型/选择性归档不是完整放行。

''' + tail, encoding='utf-8')
archive.mkdir()
records=[]
for path in sorted(paths):
    relative=path.relative_to(root)
    dest=archive/relative
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(path,dest)
    value=checksum(path)
    assert checksum(dest)==value
    records.append({'source':relative.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={
    'batch':'090','at':stamp,'status':'codex-native-execution-coordinator-migrated','goalStatus':'active',
    'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
    'rustDefaultPassed':460,'rustDefaultIgnored':168,'defaultCorePassed':304,'defaultCoreIgnored':149,'nodeTests':5,
    'codexExecutionCoordinated':True,'codexResidentRegistryShared':False,'sameProjectCodexParallelEnabled':False,
    'sameProjectOmpApiParallelEnabled':True,'runtimeHashes':runtimes,'inheritedUiSources':prior['inheritedUiSources'],
    'scopedUnitTests':3,'scopedOutcomeScenarios':5,'codexBudgetScenarios':3,'originalAdviceModes':6,'actualOwnerKills':5,
    'realModelCalls':0,'userDesktopActions':0,'newJournal':False,'executionWorkerAcceptanceAuthority':False,
    'publicDispatcherCompatibility':'old DurableWorkflow dispatch/control/reconcile/wait_supervised kept; new opaque wait_scoped has default',
    'nativeCaseResults':case_summaries,'crashResult':crash,'specializedCheckSeconds':seconds,
    'failuresPreserved':['initial generic observed_state call required explicit Box dereference; initial budget filter matched zero tests and Codex fixture env was missing; failures preserved, corrected specialized run excludes repeating successful full checks'],
    'unresolvedObservation':prior['unresolvedObservation'],'projectionSharingObservation':prior['projectionSharingObservation'],
    'remaining':['Codex shared host registry, keyed lifecycle, management/advice prefix and full heterogeneous fault matrix',
        'real OpenAI management, all first-batch access, real coding and efficiency including failures',
        'P6 install/update/rollback/migration/signing/support'],
    'previousIndex':previous.relative_to(root).as_posix(),'previousIndexSha256':checksum(previous),
    'archiveScope':'selective source and local evidence, not full reproducible repository','records':records}
with index.open('x',encoding='utf-8') as stream:
    json.dump(report,stream,ensure_ascii=False,indent=2)
    stream.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'rustPassed':460,'rustIgnored':168},ensure_ascii=False))
