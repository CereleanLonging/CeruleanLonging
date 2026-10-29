"""Freeze additive offline evidence; never turn fixture passes into release approval."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import re
import shutil
import os

root = Path(__file__).resolve().parents[1]
if os.name == 'nt' and not str(root).startswith('\\\\?\\'):
    root = Path('\\\\?\\' + str(root))
reports = root / 'reports'
index = reports / 'p5-shared-crash-20261012.json'
archive = reports / 'p5-shared-crash-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'

def checksum(path):
    return sha256(path.read_bytes()).hexdigest()

def read_log(stage):
    raw = (reports / f'p5-shared-crash-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))

previous = reports / 'p5-codex-shared-20261012.json'
prior = load(previous)
for record in prior['records']:
    assert checksum(root / record['snapshot']) == record['sha256'], record['snapshot']
allowed_changes = {
    'crates/core/examples/codex_dag_fixture.rs',
    'crates/core/src/integration_tests.rs',
    'crates/core/src/integration_tests/codex_parallel.rs',
    'crates/core/src/supervised_worker.rs',
}
inherited_sources = []
for record in prior['records']:
    name = record['source']
    if name.startswith('crates/') and name not in allowed_changes:
        assert checksum(root / name) == record['sha256'], name
        inherited_sources.append(name)
ui = load(reports / 'p5-resource-config-20261012.json')
for name in prior['inheritedUiSources']:
    record = next(r for r in ui['records'] if r['source'] == name)
    assert checksum(root / name) == record['sha256'], name
runtimes = {name: checksum(root / name) for name in prior['runtimeHashes']}
assert len(set(runtimes.values())) == 1

seconds = {}
for stage in ['crash3', 'recover1', 'supervision4', 'original-advice', 'advice-ACK', 'advice-WRONG_ACK', 'advice-LOST_ACK', 'advice-PAUSE', 'advice-PAUSE_COMMIT', 'advice-CANCEL']:
    text = read_log(stage)
    assert 'test result: ok. 1 passed; 0 failed' in text and 'test result: FAILED' not in text
    seconds[stage] = float(re.findall(r'test result: ok\..*?finished in ([\d.]+)s', text)[-1])
assert 'Finished' in read_log('clippy2') and 'error:' not in read_log('clippy2')
assert 'Finished' in read_log('fixtures2') and 'error:' not in read_log('fixtures2')
assert '305 passed; 0 failed; 155 ignored' in read_log('core')
for stage in ['build','desktop']:
    assert 'Finished' in read_log(stage) and 'error:' not in read_log(stage)

crash_dir = reports / 'p5-shared-crash-crash3-cases-20261012'
recover_dir = reports / 'p5-shared-crash-recover1-cases-20261012'
supervision_dir = reports / 'p5-shared-crash-supervision4-cases-20261012'
crash, recovery = load(crash_dir / 'result.json'), load(recover_dir / 'result.json')
assert crash['status'] == recovery['status'] == 'passed'
assert len(crash['cases']) == 8 and len(recovery['cases']) == 5
assert sum(c['peerExits'] for c in crash['cases'] + recovery['cases']) == 16
for case in crash['cases'] + recovery['cases']:
    assert case['status'] == 'passed' and case['actualOwnerKill']
    assert case['originalRecordsPreserved'] and case['unknownResourcesHeld']
    assert case['secondReopenNewEvents'] == case['promptResends'] == case['workerRestarts'] == 0
for case in recovery['cases']:
    result = case['explicitResolution']
    assert result['explicitUserAction'] and result['originalPromptResends'] == 0
    assert result['result'] == ('completed-with-original-attempts' if case['stage'] == 'two-codex-running' else 'closed-unsent-no-dispatch')
supervision = [load(supervision_dir / mode / 'result.json') for mode in ['continue', 'pause']]
for result in supervision:
    assert result['status'] == 'passed' and result['steerCalls'] == 2
    assert result['acceptedTasks'] == 3 and result['managementRequests'] == 9
    assert result['reopenNewEvents'] == result['promptResends'] == 0

stamp = datetime.now().astimezone()
handoff = root / 'handoffs/2026-10-12' / ('handoff_2026-10-12_002_' + stamp.strftime('%H%M%S%z') + '.md')
existing_drafts = list(handoff.parent.glob('handoff_2026-10-12_002_*'))
if existing_drafts:
    assert len(existing_drafts) == 1
    handoff = existing_drafts[0]
    draft = handoff.read_text(encoding='utf-8')
    assert '第001项 / 092 共享故障与恢复补证' in draft and '第002项' not in draft
    stamp = datetime.fromisoformat(next(line for line in draft.splitlines() if line.startswith('2026-10-12T')))
body = f'''092补共享调度的故障与恢复证据，并修复双任务监督的旧快照问题。执行器在准备建议前沿原受限客户端refresh读取唯一协调器，取得包含原管理快照的账本前缀；后续原turn/授权/Prepare与Commit校验保持。未增加发送权限或重试。新增两个测试模块、受控协议消息门控和检查脚本。继续复用原方案、旧接入和原账本，没有重发WorkBuddy064单次编程任务。

八个实际所有者终止窗口通过（{seconds['crash3']}秒）：Codex/API运行中、Codex/OMP运行中、双Codex运行中、API已Accept而Codex仍运行，以及共享分配Prepare前、Prepared、Assigned、Applied。总计10个测试原生子进程确认退出；原管理/事件/启动文件摘要保持，运行中任务重开为Unknown且保留资源，未派发任务不启动；两次重开第二次新增事件0、任务重发0。这是指定窗口，未覆盖所有写入中断或取消交错。

另五个新隔离场景验证明确处置（{seconds['recover1']}秒），再实际终止5个所有者、确认6个测试子进程退出。四个未发送分配窗口沿原审阅摘要关闭，释放原资源，项目保持暂停，没有模型派发；原管理决定不能借新attempt再次应用。双Codex仅回读两个原thread/turn，另一Unknown在对账期间保持锁定；原轮次恢复Submitted后，经测试用户明确恢复，三任务通过独立检查/主指挥Accept，7次管理请求、最终AB，原任务没有重发。协议远端完成内容是受控夹具，不是实际供应商的终态证明。

双Codex同时异常在继续运行及暂停后恢复两场景通过（{seconds['supervision4']}秒）：每任务两条合规失败观察，副指挥请求串行；分别绑定原attempt/thread/turn，每条建议只向自己的原轮次发送一次。各完成3任务Accepted、9次管理请求、2次steer，重开不新增事件或重发。API/OMP监督仍未开放。

失败记录保留：初次编译误用startup字段已更正；两次测试通知发布出现os32，未识别占用者，随后复用旧分配终止夹具的create_new写入与父进程完整JSON读取协议。该调整只用于测试通知，不修改生产发布规则，也不解释085投影冲突。首轮双异常测试漏了command字段，原观察器记录每任务2条rejected并停止建议，最终达到时限；补齐夹具并加入立即拒绝断言，没有放宽生产校验。随后两次合规双异常场景暴露执行器缓存落后：较早任务还持有自己的Started前缀，副指挥已引用两个任务都Started的较新前缀；原严格授权正确拒绝，执行器转Unknown、运行Attention。修复只增加准备前受限只读刷新，失败快照及日志保留。严格clippy首次发现新增测试对Copy类型多余clone，删除后通过；该纯测试语义等价修正不触发整套基础接入重跑。

本批新专项、原共享建议ACK三个窗口、原单执行者ACK/错误ACK/丢失ACK/暂停/Commit前暂停/取消六种组合、核心305项（155项显式忽略）、严格workspace全targets clippy、核心及桌面custom-protocol debug构建通过。非核心156项、Node5项及前端构建继承091，未重跑基础接入；本批新增3个显式忽略项为终止父子辅助与双监督测试。13次终止与明确恢复结果先于建议刷新修复，相关场景均未启用监督，沿原证据继承，不重复杀进程；并不代证建议事务的全部故障矩阵。冻结前核对091的{len(prior['records'])}份归档、被继承源码及079界面。三个更新runtime摘要一致：{next(iter(runtimes.values()))}。新归档为选择性证据，非完整可复现仓库。

完整P4/P5/P6仍未验收，目标active。真实模型调用及用户桌面动作0。仍缺共享清理不确定/部分写入/取消与验收交错/负载完整矩阵，真实OpenAI主副指挥、全部首批API/CLI/桌面、真实编程与包含失败的效率。P6仍需干净安装卸载、升级失败恢复、备份迁移、回滚、签名和支持矩阵；新schema2/binding2被旧程序拒绝，不能称降级已兼容。075 Job退出和085占用方未定结论保留，金额硬预算及分执行者监督策略待完成。
'''
latest = root / 'docs/HANDOFF-LATEST.md'
old = latest.read_text(encoding='utf-8-sig')
marker = '当前：2026-10-12 卷001第020项（091）'
assert marker in old
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第001项（092），progress，092最终检查已通过。\n\n' + stamp.isoformat() + '\n\n' + body + '\n[092验收索引](../reports/p5-shared-crash-20261012.json)。\n\n下方091为冻结历史。\n\n' + old[old.index(marker):], encoding='utf-8')
handoff.write_text('# 交接文档 2026-10-12 卷002\n\n## 第001项 / 092 共享故障与恢复补证\n\n' + stamp.isoformat() + '\n\n' + body + '\n[092验收索引](../../reports/p5-shared-crash-20261012.json)。\n', encoding='utf-8')
doc_names = ['docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md']
for name in doc_names:
    p = root / name
    text = p.read_text(encoding='utf-8-sig').replace('更新至批次091', '更新至批次092').replace('本页091小节', '本页092小节')
    text = text.split('\n\n## 092 共享故障与恢复补证（本批通过，完整目标active）')[0]
    p.write_text(text + '\n\n## 092 共享故障与恢复补证（本批通过，完整目标active）\n\n' + stamp.isoformat() + '\n\n' + body + '\n[092验收索引](../reports/p5-shared-crash-20261012.json)。\n', encoding='utf-8')
gates = root / 'docs/NEXT-STAGE-GATES.md'
old = gates.read_text(encoding='utf-8-sig')
prefix = '''# 后续阶段的验收执行顺序

2026-10-12 / 092 更新：八个共享所有者终止窗口、五个明确处置场景、双原生任务同时监督两场景已通过。新增建议准备前的受限账本刷新，原授权校验保持；见[最新交接](HANDOFF-LATEST.md)与[092索引](../reports/p5-shared-crash-20261012.json)。完整P4/P5/P6未通过。

- 继续补共享生命周期的清理不确定、部分写入、取消与独立验收交错、错位回执和负载；不要重复已通过的基础接入或把指定窗口当作全部故障矩阵。
- 原schema2/binding2兼容边界、API/OMP监督未开放、金额硬预算待接入保持。075 Job退出与085投影共享冲突占用方未定。
- 按原实施方案完成真实OpenAI管理、全部首批API/CLI/桌面、真实编程及含失败的效率。WorkBuddy064原单次授权已经消费，不重发sum任务。
- P6完整门槛保持：干净安装/卸载、升级及失败恢复、备份迁移、回滚、签名和支持矩阵；受控协议测试及debug构建不代替正式验收。
- 已开启本日卷002第001项，旧卷001二十项保持不动。

'''
gates.write_text(prefix + old[old.index('以下为较早阶段的历史记录。'):], encoding='utf-8')

paths = {root / r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
paths.update(root / n for n in ['crates/core/src/integration_tests/shared_crash.rs', 'crates/core/src/integration_tests/shared_supervision.rs', 'scripts/check-shared-crash-20261012.ps1', 'scripts/finalize-shared-crash-20261012.py', 'AGENTS.md', 'docs/HANDOFF-LATEST.md', 'docs/NEXT-STAGE-GATES.md', *doc_names])
paths.add(handoff)
paths.update(reports.glob('p5-shared-crash-*-20261012.log'))
case_dirs = [crash_dir, recover_dir, supervision_dir, reports/'p5-shared-crash-crash1-cases-20261012', reports/'p5-shared-crash-crash2-cases-20261012', reports/'project-integration-THyBuR', reports/'p5-shared-crash-supervision2-cases-20261012', reports/'p5-shared-crash-supervision3-cases-20261012']
case_dirs.extend(reports.glob('p5-shared-crash-advice-*-cases-20261012'))
for base in case_dirs:
    assert base.is_dir(), base
    for p in base.rglob('*'):
        rel = p.relative_to(base)
        if (p.is_file() and rel.parts[0] not in {'pool', 'project', 'repo', 'target'}
                and not any(part in {'.git', 'credentials'} for part in rel.parts[:-1])):
            if p.suffix in {'.json', '.jsonl', '.pending', '.stdout', '.stderr'} or p.name.startswith(('gated-', 'release-')):
                paths.add(p)
archive.mkdir()
records = []
for p in sorted(paths):
    assert p.is_file(), p
    relative = p.relative_to(root)
    dest = archive / relative
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(p, dest)
    value = checksum(p)
    assert checksum(dest) == value
    records.append({'source': relative.as_posix(), 'snapshot': dest.relative_to(root).as_posix(), 'sha256': value})
report = {'batch':'092', 'at':stamp.isoformat(), 'status':'specified-shared-crash-recovery-and-simultaneous-supervision-verified', 'goalStatus':'active',
          'fullP4Accepted':False, 'fullP5Accepted':False, 'fullP6Accepted':False, 'productionChanged':True, 'productionChange':'scoped refresh before worker advice prepare; original authorization checks retained',
          'realModelCalls':0, 'userDesktopActions':0, 'actualOwnerKills':13, 'uniqueCrashWindowTypes':8, 'confirmedNativePeerExits':16,
          'crashResult':crash, 'explicitRecoveryResult':recovery, 'supervisionResults':supervision,
          'specializedCheckSeconds':seconds, 'runtimeHashes':runtimes,
          'defaultChecks':{'corePassed':305,'coreIgnored':155,'coreRerun':True,'inheritedNonCoreRustPassed':156,'nodePassed':5,'inheritedBatch':'091'}, 'builds':['core','desktop-custom-protocol-debug'],
          'newExplicitlyIgnoredTests':3, 'strictClippy':'passed', 'inheritedProductionSources':inherited_sources,
          'inheritedUiSources':prior['inheritedUiSources'], 'previousIndex':previous.relative_to(root).as_posix(), 'previousIndexSha256':checksum(previous),
          'archiveScope':'selective source and fixture evidence; not a complete reproducible repository',
          'remaining':['full heterogeneous shared lifecycle/fault/load matrix','real OpenAI management, all first-batch access and real coding/efficiency including failures','P6 install/update/rollback/migration/signing/support'],
          'records':records}
with index.open('x', encoding='utf-8') as f:
    json.dump(report, f, ensure_ascii=False, indent=2)
    f.write('\n')
print(json.dumps({'index':str(index), 'records':len(records), 'handoff':str(handoff), 'fullP6Accepted':False}, ensure_ascii=False))
