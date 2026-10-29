"""Freeze 091 after original regression and new shared-host gates pass."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil

root = Path(__file__).resolve().parents[1]
reports = root / 'reports'
index = reports / 'p5-codex-shared-20261012.json'
archive = reports / 'p5-codex-shared-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def checksum(path):
    return sha256(path.read_bytes()).hexdigest()
def read(stage):
    raw = (reports / f'p5-codex-shared-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')
previous = reports / 'p5-codex-coordinator-20261012.json'
prior = json.loads(previous.read_text(encoding='utf-8'))
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
ui = json.loads((reports / 'p5-resource-config-20261012.json').read_text(encoding='utf-8-sig'))
for name in prior['inheritedUiSources']:
    record = next(r for r in ui['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
for name in ['crates/core/src/api_execution.rs', 'crates/core/src/coordinated_codex.rs',
             'crates/core/src/coordinated_omp.rs', 'crates/core/src/workflow_coordinator.rs',
             'crates/core/src/git_workspaces.rs', 'crates/core/src/verifier.rs',
             'crates/core/src/verifier/process.rs', 'crates/core/src/omp_process.rs']:
    record = next(r for r in prior['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
full = read('full')
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored', full)
passed = sum(int(p) for p, _ in counts)
ignored = sum(int(i) for _, i in counts)
assert passed == 461 and ignored == 171, counts
assert '305 passed; 0 failed; 152 ignored' in full
assert 'test result: FAILED' not in full and 'pass 2' in full and 'pass 3' in full and 'built in' in full
assert '305 passed; 0 failed; 152 ignored' in read('core3')
checks = {'automatic':1, 'supervision2':1, 'omp-parallel':2, 'worktree2':4,
          'dag-success':1, 'dag-recover-a':1, 'crash':1}
for mode in ['ACK','WRONG_ACK','LOST_ACK','PAUSE','PAUSE_COMMIT','CANCEL']:
    checks['advice-'+mode]=1
for stage,count in checks.items():
    assert f'{count} passed; 0 failed' in read(stage), stage
for stage in ['fixtures2','build','clippy','desktop']:
    assert 'Finished `dev`' in read(stage), stage
for pair in ['Codex/Deepseek','Deepseek/Codex','Codex/Omp','Codex/Codex']:
    for cancel in ['false','true']:
        assert f'Codex parallel {pair} cancel={cancel}:' in read('automatic')
# The initial combined run failed only its automatic test; its independent ACK
# test passed all three windows. Preserve and explicitly attribute both results.
ack_log=read('parallel')
for window in ['prepare','commit','lost-commit']:
    assert f'Codex shared ACK {window}:' in ack_log
    assert f'Codex shared advice {window}:' in read('supervision2')
assert '1 passed; 1 failed;' in ack_log
assert 'automatic_codex_mixed_parallel_completion_and_single_attempt_cancel ... FAILED' in ack_log
assert 'Codex shared ACK lost-commit:' in ack_log and '\nok\n' in ack_log.replace('\r\n','\n')
case_paths=[]
cases=[]
for mode in ['success','recover-a']:
    base=reports/f'p5-codex-shared-dag-{mode}-cases-20261012'
    result=json.loads((base/'result.json').read_text(encoding='utf-8'))
    assert result['status']=='passed' and result['realModelCalls']==0 and result['finalRestartNewEvents']==0
    cases.append(result)
    case_paths.extend(base.glob('*.json'))
    case_paths.append(base/'bin/protocol.jsonl')
for mode in ['ACK','WRONG_ACK','LOST_ACK','PAUSE','PAUSE_COMMIT','CANCEL']:
    base=reports/f'p5-codex-shared-advice-{mode}-cases-20261012'
    result=json.loads((base/'delivery-result.json').read_text(encoding='utf-8'))
    assert result['status']=='passed' and result['realModelCalls']==0 and result['newEventsAfterRestart']==0
    cases.append({k:v for k,v in result.items() if k not in ['snapshot','details']})
    case_paths.extend(base.glob('*.json'))
    case_paths.append(base/'bin/protocol.jsonl')
base=reports/'p5-codex-shared-crash-cases-20261012'
crash=json.loads((base/'crash-result.json').read_text(encoding='utf-8'))
assert crash['status']=='passed' and len(crash['cases'])==5
assert all(c['realTermination'] and c['peerExited'] and c['realModelCalls']==0 for c in crash['cases'])
case_paths.append(base/'crash-result.json')
for point in ['prepared','advised','intent','received','acknowledged']:
    case_paths.extend(base/point/name for name in ['snapshot.json','bin/protocol.jsonl','owner.stdout','owner.stderr'])
runtimes={name:checksum(root/name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values()))==1
seconds={stage:float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s',read(stage))[-1]) for stage in checks}
stamp=datetime.now().astimezone().isoformat()
body='''091将Codex加入既有API/OMP共享登记表，原账本仍由单写协调器持有。活动执行、取消、建议和管理确认按(project,attempt)定位；Done仅返回结果、中断及进程清理证明。API、OMP、Codex或独立验收仍有使用者时，原所有者不会提前归还。进程清理未确认或通道消失保留Unknown活动与资源，不把线程结束当作进程已退出。项目投影来自协调器，迟到的Codex Progress不能回退或重复项目。

批准的独立worktree任务现在可自动选择Codex/API、API/Codex、Codex/OMP以及两个Codex并行。原模型槽2、后台操作4、预算事务、管理上下文、Git与独立验证器保持；原生取消仅发送给原attempt，同时按原语义暂停后续调度，其他已发送任务继续收集。任务A可在B未结束时完成独立检查和主指挥Accept，随后最终集成仍遵守依赖。

受管理的Codex独立目录启动改用原StartupIntent schema2，不可变前缀与Prepare/Commit复用089机制。原schema1和无StartupJournal的旧Codex管理记录仍走原严格快照桥接，未授予共享权限。新监督Binding version2绑定原方案/批准、任务/attempt、Started thread/turn及原观察；只允许其他任务的限定事件交错。建议沿原管理应用、原Advise事件与原delivery journal发送，未增加第二账本，执行器不能批准、Verify或Accept。

新离线生产路径覆盖8种自动并行/单尝试取消场景，正常各3任务Accepted、7次管理请求、最终AB；取消保留另一任务Submitted、集成零派发。错误attempt拒绝、迟到快照不回退、每任务一次派发、重开零新增事件。管理ACK三窗口覆盖API与Codex，建议ACK三窗口使用双Codex，分别覆盖Prepare前/Commit前其他任务完成及Commit回执丢失；伪造前缀拒绝，原授权与原轮次保持，失去确认不重发。建议正常仅向原B轮次一次steer并独立验收收口；丢失确认保留原Advise/未知执行，不补发建议。

初轮默认检查的六处失败来自旧夹具仍把活动缓存当作项目权威、schema2旧拒绝断言，以及旧无StartupJournal管理记录。已将夹具接入原共享所有者，并保留旧记录的严格桥接路径；没有放宽新schema2校验。新增自动专项首次把等待模型响应的API误要求为Running而超时，已按原Dispatching语义修正；该次独立管理ACK三场景已通过并继承。监督专项首次试用API/Codex混合方案，被原全局监督策略正确拒绝；改用原支持的纯Codex方案验证双原生任务交错，没有开放API/OMP监督。所有失败日志保留，未重跑WorkBuddy064或原桌面基础接入。

原工作目录专项又发现实际竞态：协调器Started已入账并进入项目投影，但取消审阅仍读取较旧活动缓存，短暂没有取消入口。取消审阅现从原协调器读取同一权威快照，证据摘要与实际审阅一致；旧无登记表夹具仍保留原回退。原实进程测试刻意保留Pending缓存，验证Started后的取消不需要额外轮询；工作目录专项与305项核心、剩余取消/恢复专项重新通过。非核心/前端/Node及先前新增并行和监督专项继承本批已通过结果，不重跑整套基础接入。
'''
body+=(f'\n最终检查：{passed}项默认Rust/{ignored}项显式忽略（核心305/152）、5项Node、前端/核心构建通过；'
       f'自动并行8场景{seconds["automatic"]}秒、建议交错3场景{seconds["supervision2"]}秒、原OMP并行/ACK、Codex工作目录4项、两依赖DAG、六纠偏组合及五实际owner终止窗口通过。'
       f'最终核心305项对应取消竞态修复后的源码，非核心与前端/Node继承本批全量；严格workspace全targets clippy及桌面custom-protocol debug构建通过；三个runtime摘要一致：{next(iter(runtimes.values()))}。\n\n'
       f'原090的{len(prior["records"])}份冻结归档和079界面摘要保持。真实模型调用/用户桌面动作0。原五终止窗口复用非共享场景，不能当作新版异构共享全部崩溃矩阵。'
       '旧程序拒绝新Codex启动schema2及监督binding2，未证明P6降级兼容；API/OMP监督未开放，混合方案不能统一启用当前全局监督策略，金额硬预算仍待接入。'
       '075 Job退出观察、085投影共享冲突占用方未定结论保持。完整P4/P5/P6尚未验收，目标active。'
       '下一步补异构共享生命周期/故障/负载和真实OpenAI管理、全部首批接入、真实编程及包含失败的效率，再完成P6干净安装/升级/回滚/迁移/签名/支持矩阵。\n')
latest=root/'docs/HANDOFF-LATEST.md'
old=latest.read_text(encoding='utf-8-sig')
marker='当前：2026-10-12 卷001第019项（090）'
assert marker in old and '091最终检查已通过' not in old
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷001第020项（091），progress，091最终检查已通过。上一轮090为progress。\n\n'+stamp+'\n\n'+body+'\n[091验收索引](../reports/p5-codex-shared-20261012.json)。\n\n下方090为冻结历史。\n\n'+old[old.index(marker):],encoding='utf-8')
doc_names=['docs/P5-WORKTREE-IMPLEMENTATION.md','docs/STAGE-ACCEPTANCE-2026-10-12.md','handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md']
for name in doc_names:
    p=root/name
    original=p.read_text(encoding='utf-8-sig')
    assert '091 Codex共享宿主' not in original
    original=original.replace('更新至批次090','更新至批次091').replace('本页090小节','本页091小节')
    prefix='../../' if name.startswith('handoffs/') else '../'
    heading='第020项 / ' if name.startswith('handoffs/') else ''
    p.write_text(original+'\n\n## '+heading+'091 Codex共享宿主（本批检查通过，完整目标active）\n\n'+stamp+'\n\n'+body+'\n[091验收索引]('+prefix+'reports/p5-codex-shared-20261012.json)。\n',encoding='utf-8')
gates=root/'docs/NEXT-STAGE-GATES.md'
old=gates.read_text(encoding='utf-8-sig')
gates.write_text('''# 后续阶段的验收执行顺序

2026-10-12 / 091 更新：Codex与API/OMP已共用原协调器，公开自动并行、指定attempt取消、管理/建议交错确认和独立收口通过本批差异检查。先读[最新交接](HANDOFF-LATEST.md)、[当日矩阵](STAGE-ACCEPTANCE-2026-10-12.md)与[P5记录](P5-WORKTREE-IMPLEMENTATION.md)。完整P4/P5/P6尚未通过。

- 下一步补共享生命周期故障矩阵：进程清理未确认、所有者真实终止、错位/丢失回执、两原生尝试同时产生监督事件、并发负载及共享恢复。新8种正常/取消组合、API/Codex管理ACK和双Codex单目标监督ACK交错已通过；旧五终止窗口仍为非共享场景，不扩大结论。
- 原启动schema2现在支持受管理的Codex/OMP独立目录；监督Binding2允许其他任务限定事件交错。旧1保持，旧程序拒绝新记录，须纳入P6迁移/回滚矩阵。
- 原模型槽2、后台操作4和API全轮数/CLI1次预算保持，Unknown保留额度。原执行器不能批准、Verify或Accept；API/OMP监督未开放，当前全局策略不能在混合方案启用；按任务能力配置监督和金额硬预算仍待接入。
- 真实OpenAI主副指挥、全部首批API/CLI/桌面接入、真实异构编程与包含失败的效率仍需验收。先复用预案证据；WorkBuddy064单次授权已消费，不重发原sum任务。
- 075 Job退出观察、085投影共享冲突占用方未定结论保留。P6需干净安装/卸载、升级/失败恢复、备份迁移、回滚、签名及支持矩阵；debug构建与受控模型不代替正式放行。
- 本日卷001已到第020项；下一批按既有分卷规则另建卷002并写时间戳。

'''+old[old.index('以下为较早阶段的历史记录。'):],encoding='utf-8')
names={r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
names.update(str(p.relative_to(root)).replace('\\','/') for p in (root/'crates/core/src').rglob('*.rs'))
names.update(['crates/core/examples/codex_dag_fixture.rs','AGENTS.md','scripts/check-codex-shared-20261012.ps1','scripts/finalize-codex-shared-20261012.py','scripts/migrate-codex-shared-20261012.py','scripts/migrate-codex-supervision-20261012.py','scripts/migrate-codex-shared-fixtures-20261012.py','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md',*doc_names])
paths={root/n for n in names}|set(case_paths)|set(reports.glob('p5-codex-shared-*-20261012.log'))
assert all(p.is_file() for p in paths)
archive.mkdir()
records=[]
for p in sorted(paths):
    relative=p.relative_to(root);dest=archive/relative
    dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
    value=checksum(p);assert checksum(dest)==value
    records.append({'source':relative.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'091','at':stamp,'status':'codex-shared-host-parallel-and-scoped-supervision-verified','goalStatus':'active',
        'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
        'rustDefaultPassed':passed,'rustDefaultIgnored':ignored,'defaultCorePassed':305,'defaultCoreIgnored':152,'nodeTests':5,
        'codexResidentRegistryShared':True,'sameProjectCodexParallelEnabled':True,'sameProjectOmpApiParallelEnabled':True,
        'automaticScenarios':8,'assignmentAckScenarios':3,'supervisionAckScenarios':3,'actualOwnerKills':5,
        'actualOwnerKillScope':'inherited exclusive-task scenarios, not full new shared-owner crash matrix',
        'realModelCalls':0,'userDesktopActions':0,'newJournal':False,'executionWorkerAcceptanceAuthority':False,
        'runtimeHashes':runtimes,'inheritedUiSources':prior['inheritedUiSources'],'nativeCaseResults':cases,'crashResult':crash,
        'specializedCheckSeconds':seconds,'assignmentAckEvidence':'independent passing test in preserved parallel log; separate automatic test initially failed and corrected',
        'finalCoreEvidence':'core3 after cancellation authoritative-snapshot fix; non-core/frontend/Node inherited from full',
        'cancellationRaceFixEvidence':['worktree2 deterministic stale Pending activity cache with actual Started ledger','core3','advice-CANCEL'],
        'unresolvedObservation':prior['unresolvedObservation'],'projectionSharingObservation':prior['projectionSharingObservation'],
        'remaining':['complete shared-owner heterogeneous crash/lifecycle/load matrix','real OpenAI management, all first-batch access, real coding and efficiency including failures','P6 install/update/rollback/migration/signing/support'],
        'previousIndex':previous.relative_to(root).as_posix(),'previousIndexSha256':checksum(previous),
        'archiveScope':'selective source and evidence; not a complete reproducible repository','records':records}
with index.open('x',encoding='utf-8') as f:
    json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'rustPassed':passed,'rustIgnored':ignored},ensure_ascii=False))
