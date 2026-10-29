"""Freeze checkpoint 056 once after local reply contract and regressions."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-proof-review-20261010.json'
archive = reports / 'p4-workbuddy-proof-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

previous = {}
labels = ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review', 'stop-review-crash', 'joint', 'joint-ui']
old_paths = [f'reports/p4-desktop-{label}-review-20261010.json' for label in labels]
old_paths += [f'reports/p4-workbuddy-{label}-review-20261010.json' for label in ['composer', 'model', 'history']]
for path in old_paths:
    old = load(path)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[path] = len(old['records'])

jobs, directories = {}, []
for name in ['workbuddy-proof-owned-20261010', 'workbuddy-proof-history-owned-20261010']:
    directory = reports / name
    job = load(directory / 'cleanup.json')
    assert job['ExitCode'] == 0 and job['CleanupConfirmed'] and job['RemainingProcesses'] == 0
    jobs[name] = job
    native = Path(json.loads(log(directory / 'stdout.log').strip())['report'])
    assert native.is_relative_to(reports)
    directories.extend([directory, native])
reply = load('reports/workbuddy-reply-nIm5r1/result.json')
assert reply['status'] == 'passed' and len(reply['cases']) == 21 and reply['rustContractPassed']
history = load('reports/workbuddy-history-52bOcR/result.json')
assert history['status'] == 'passed' and history['liveRounds'] == 4 and history['repeatedObservationMatches']
checks = log('reports/p4-workbuddy-proof-check.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (419, 0, 95)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
assert 'Finished' in log('reports/p4-workbuddy-proof-clippy.log')
assert 'Finished' in log('reports/p4-workbuddy-proof-desktop-build.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({v['sha256'] for v in runtime}) == 1
(reports / 'p4-workbuddy-proof-runtime.json').write_text(json.dumps(runtime, indent=2) + '\n', encoding='utf-8')
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert not daily.exists(), 'Daily handoff already exists'
entry = f'''

## 056 WorkBuddy本地回复证据与跨语言校验 {stamp}

上轮及本轮progress，目标active，完整P0–P6未通过。承接10月10日055；本批内部编号056，10月11日本日第001项、本卷第001项。代码与专项验证完成于10月10日，本次补完完整回归后封存。

DesktopTaskProof的sessionId改为可空，新增可选localBinding（原关联ID、原任务审阅摘要、当前完整观察）。WorkBuddy分支要求null供应商ID、正确5.7.7版本、同进程/窗口/文档/模型、空草稿/空闲、时间一致、至少两轮原历史后缀和最终完整轮摘要。调用此回复校验前仍需既有批准/持久派发验证。Qoder分支保持供应商ID要求且拒绝本地证据混入，旧JSON字符串及省略新字段的字节往返保持；未宣称完整产品迁移验收。

生产C#只读taskread接055的WorkBuddy结构解析，原提示词必须唯一且位于最后完整成功轮，拒绝引用标记、取消/未完成轮、重复用户文本、改历史及窗口/文档/模型变化。该分支不发送或停止。WorkBuddy tasksend、stop、带前序继续以及产品宿主派发门槛仍未开放，不能把读取器当调度完成。

reports/workbuddy-reply-nIm5r1：21个生产C#合成回复场景通过，含裁剪历史、跨身份拒绝和发送/停止仍拒绝；同一真实测试进程的C#输出由Rust TaskReview/TaskProof反序列化、校验、再次序列化后复核通过。合成Review没有真实用户批准/工作流，因此只证明回复契约，不证明任务派发、真实模型或交付。

新增2个默认Rust测试覆盖22种WorkBuddy证据篡改拒绝与Qoder旧格式兼容；另1显式跨语言进程契约测试通过。实际WorkBuddy原聊天只读回归reports/workbuddy-history-52bOcR通过，19解析场景及两次4轮历史/观察一致；没有消息、模型或文件任务调用。未操作Qoder。

完整scripts/check.ps1通过：419默认Rust/0失败/95忽略、5Node、fmt、契约/前端/核心构建；全workspace/all-targets/custom-protocol严格clippy与桌面debug构建通过，三runtime一致。首次沙箱检查在resident_client命名管道连接处报拒绝访问，失败日志单独保留；上次沙箱外复验因自动审批额度未执行，恢复时间后重新审批并执行。两个专项测试Job退出0/回收后0。041–055旧索引有效，本批选择性源码与证据见../../reports/p4-workbuddy-proof-review-20261010.json。无HEAD/推送/云CI；本轮未操作用户应用，10月10日实机证据不代表本日窗口状态。

下一步继续WorkBuddy原任务取消观察与本地绑定、一次性发送/停止动作保护及受控UIA验证，再接宿主/UI/调度与实机编程验收。真实OpenAI管理、其余恢复专项、全部首批API/CLI、P5异构并行和P6完整范围仍保留。不要重跑已完成finalize脚本。
'''
daily.parent.mkdir(parents=True, exist_ok=True)
daily.write_text('# 2026-10-11 交接卷001\n\n承接[10月10日卷003](../2026-10-10/handoff_2026-10-10_003_102620+0800.md)。\n' + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第001项（批次056）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [WorkBuddy动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-proof-review-20261010.json)

WorkBuddy只读任务回复分支与本地关联证据已实现，sessionId保持null。必须核对关联、原任务审阅、窗口/文档/模型及历史后缀；Qoder旧回复序列化保持兼容。21个合成C#场景、22种Rust篡改拒绝、实际C#→Rust契约验证通过。WorkBuddy实机两次4轮只读历史回归通过，真实任务消息/模型调用0。

419默认Rust/95忽略、5Node、fmt/严格clippy、前端/核心/桌面构建通过，三runtime一致；两个Job回收后0。041–055旧索引有效。

下一步：WorkBuddy原任务取消观察、本地关联和一次性发送/停止保护，再接宿主/UI/调度及实机编程。当前发送/停止/连续任务与产品派发门槛仍未放开；真实OpenAI管理、其他恢复专项、全部首批API/CLI、P5/P6未验收，目标active。本日/本卷1项；无HEAD/推送/云CI，不要重跑finalize脚本。使用实机前重新发现窗口；本批实机报告采于10月10日。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-11.md'
assert not matrix.exists()
matrix.write_text(f'''# 阶段验收评估 2026年10月11日

继承[10月10日矩阵](STAGE-ACCEPTANCE-2026-10-10.md)全部范围与缺口，完整P0–P6尚未通过。

{stamp} 本日001（内部批次056）：WorkBuddy本地回复证据与只读taskread分支实现，21个C#合成场景/22种Rust变更拒绝及跨语言契约通过；10月10日实机4轮历史回归通过。419默认Rust/95忽略、5Node和完整检查构建通过。

P4仍待WorkBuddy发送/停止/调度与真实编程、Qoder真实交付、其他恢复专项及竞争/升级验证。P2真实OpenAI主副指挥全链、P3全部首批API/CLI、P5异构worktree并行及P6干净机器安装/升级/回滚/迁移/签名仍待验收。见[056交接](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)与[本批证据](../reports/p4-workbuddy-proof-review-20261010.json)。
''', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'crates/core/src/desktop_execution.rs', 'crates/core/src/desktop_local_proof_tests.rs', 'crates/core/src/desktop_sessions_tests.rs', 'crates/domain/src/desktop_sessions.rs', 'crates/domain/src/bin/export-contracts.rs', 'apps/desktop/src/contracts.ts', 'tests/workbuddy-reply.mjs', 'tests/workbuddy-history.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-proof-20261010.py', 'docs/WORKBUDDY-ACTION-CONTRACT-V1.md', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-proof-*.log'))
paths.add(reports / 'p4-workbuddy-proof-runtime.json')
result = {'checkpoint':56, 'createdAt':datetime.now(timezone.utc).isoformat(), 'status':'partial-stage-progress', 'nativeReplyScope':'synthetic production C# reply to Rust contract', 'nativeReplyCases':21, 'rustMutationCases':22, 'liveHistoryRounds':4, 'externalMessages':0, 'realModelCalls':0, 'fullRust':{'passed':419,'failed':0,'ignored':95}, 'nodeTests':5, 'jobs':jobs, 'runtime':runtime, 'previousIndexVerifiedCounts':previous, 'p4':'incomplete', 'p6':'incomplete', 'sourceArchive':'selected files only; not a full reproducible build', 'records':[{'path':p.relative_to(root).as_posix(),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint':56,'records':len(result['records']),'goal':'active'}))
