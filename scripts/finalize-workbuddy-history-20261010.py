"""Freeze checkpoint 055 once, preserving successful and failed native evidence."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-history-review-20261010.json'
archive = reports / 'p4-workbuddy-history-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review', 'stop-review-crash', 'joint', 'joint-ui']:
    p = f'reports/p4-desktop-{label}-review-20261010.json'
    old = load(p)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[label] = len(old['records'])
for label in ['composer', 'model']:
    old = load(f'reports/p4-workbuddy-{label}-review-20261010.json')
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous['workbuddy-' + label] = len(old['records'])

live_dir = reports / 'workbuddy-history-iQxdWc'
live = load(live_dir / 'result.json')
assert live['status'] == 'passed' and len(live['cases']) == 19
assert all(c['passed'] for c in live['cases'])
assert live['liveRounds'] == 4 and live['repeatedReadMatches'] and live['repeatedObservationMatches']
assert live['continuationRejected'] and live['readOnly'] and live['messagesSent'] == live['realModelCalls'] == 0
assert live['liveObservation']['modelLabel'] == 'Hy3' and live['liveObservation']['sessionId'] is None
native = load('reports/desktop-workbuddy-composer-xQYfe3/result.json')
assert native['status'] == 'passed' and len(native['cases']) == 19
assert all(c['passed'] and c['inputChanges'] == 0 for c in native['cases'])
navigation = load('reports/workbuddy-navigate-test-PQm1Yj/result.json')['response']['result']
assert navigation['messagesSent'] == 0 and navigation['testLabels'] == 2
jobs = {}
directories = [p for p in reports.glob('workbuddy-history-*') if p.is_dir()]
for directory in directories:
    if (directory / 'cleanup.json').exists():
        job = load(directory / 'cleanup.json')
        assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0, directory
        jobs[directory.name] = job
assert len(jobs) == 15
assert jobs['workbuddy-history-live-owned-20261010-04']['ExitCode'] == 0
for name in ['desktop-workbuddy-composer-xQYfe3', 'desktop-live-observation-8OzIAj', 'desktop-live-observation-sLNTgL', 'desktop-capability-diagnostic-MADEFa', 'desktop-capability-diagnostic-407GeK', 'workbuddy-navigate-test-Q06IPc', 'workbuddy-navigate-test-2ANiWU', 'workbuddy-navigate-test-PQm1Yj']:
    directories.append(reports / name)

checks = log('reports/p4-workbuddy-history-check.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (417, 0, 94)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
assert 'Finished' in log('reports/p4-workbuddy-history-clippy.log')
assert 'Finished' in log('reports/p4-workbuddy-history-desktop-build.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({v['sha256'] for v in runtime}) == 1
(reports / 'p4-workbuddy-history-runtime.json').write_text(json.dumps(runtime, indent=2) + '\n', encoding='utf-8')
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 055 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 055 WorkBuddy只读历史准备与导航结果核查 {stamp}

本轮progress，目标active，完整P0–P6未通过。本日55项、本卷15项。

生产DesktopTrial增加WorkBuddy 5.7.7.0无前序TaskReview的只读taskprepare：先验证空闲/无草稿及唯一模型，再从当前Document解析内容区标题、用户/助手配对、独立状态与正文。允许单个裁剪助手前缀，其余至少两轮完整历史；拒绝缺口、重复、歧义、多正文、引用终态及不完整尾轮。只返回轮次摘要；WorkBuddy发送、停止、带前序继续与宿主调度门槛保持拒绝。

19项合成正文/真实采集结构解析测试通过；19项真实WPF/UIA输入框/模型保护回归通过，增加已选模型但缺少历史的准备拒绝。测试种类分别记账，不把合成文本当实机聊天证据。

用户明确同意重新启动并验证，启动D:/workbuddy/WorkBuddy.exe，PID21352。早期发现缺窗口/输入框后，加载完成可读首页。旧脚本Invoke只聚焦侧栏但返回成功（testLabels=1），这是验证脚本假阳性；增加精确内容标题后未确认导航/发生超时，未当通过。截图已查看，确为首页。最终在两次空闲同观察、前台身份与实时覆盖窗口检查后，对现有“工作台接入验证”行单次物理点击成功，testLabels=2、Hy3。没有新建聊天、输入或发送消息。

最终原生桥接实机证据reports/workbuddy-history-iQxdWc/result.json：两次读取4轮，各轮SHA及窗口、Document、模型、content/layout摘要、节点数和状态一致。sessionId保持null，前序TaskReview拒绝且结果已保存。该桥接由生产C#三文件直接编译，尚未接产品宿主任务派发；不等于稳定供应商会话认证。消息与模型调用0，未读WorkBuddy Document.Value/账号路由，聊天正文未落盘。Qoder只读观察仍忙碌，未操作。

scripts/check.ps1通过：417默认Rust/0失败/94忽略、5Node、fmt、契约/前端/核心构建；全workspace/all-targets/custom-protocol严格clippy和桌面debug构建通过。三runtime摘要一致。15个测试Job均回收后0，包含保留的失败/超时运行，不把全部Job算测试通过；最终实机、解析器和原生保护运行退出0。用户WorkBuddy保留运行，未终止用户应用。

041–054旧索引全部校验，新增索引../../reports/p4-workbuddy-history-review-20261010.json与选择性源码归档。无HEAD提交/推送/云CI。下一步实现WorkBuddy本地binding身份、原任务回复/停止证据与一次性动作保护，再接宿主/UI/调度；真实OpenAI主副指挥、指定聊天真实编程、其余恢复专项、全部首批API/CLI、P5异构并行和P6完整范围仍保留。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第015项（本日055）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [WorkBuddy动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-history-review-20261010.json)

WorkBuddy已按用户授权启动，进入既有“工作台接入验证”。修复专用导航验证脚本将Invoke返回当成成功的问题；现在要求实际内容标题。生产桥接两次只读历史准备通过，4轮摘要及窗口/文档/模型/内容/布局一致，模型Hy3、sessionId仍null，前序继续拒绝。没有消息或模型调用。

19项解析测试和19项真实UIA输入框/模型保护回归通过；417默认Rust/94忽略、5Node、fmt/严格clippy及前端/核心/桌面构建通过，三runtime一致。15个Job（含失败调查）均回收后0；用户WorkBuddy保留，Qoder仍忙碌未操作。041–054旧索引有效。

下一步：WorkBuddy本地binding回复/停止证据、一次性动作保护，再接宿主/UI/调度。真实第三方编程、真实OpenAI管理全链、其余恢复专项、全部首批API/CLI、P5/P6仍未验收，目标active。本卷已有15项。无HEAD/推送/云CI；不要重跑finalize脚本。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
matrix.write_text(matrix.read_text(encoding='utf-8').rstrip() + f'\n\n{stamp} 第055项：WorkBuddy 5.7.7原生桥接实机只读准备通过，两次4轮历史/观察一致，sessionId保持null。19解析测试、19真实UIA保护回归及417默认Rust/94忽略、5Node、完整检查构建通过。导航成功须实际内容标题，原Invoke假阳性已修复。15Job含失败调查均回收后0。任务发送/取消/本地binding与调度、真实编程及完整P4/P5/P6仍待验收。见055交接与reports/p4-workbuddy-history-review-20261010.json。\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'tests/workbuddy-history.mjs', 'tests/workbuddy-navigate-test.mjs', 'tests/desktop-workbuddy-composer.mjs', 'tests/fixtures/WorkBuddyComposerWindow.cs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-history-20261010.py', 'docs/WORKBUDDY-ACTION-CONTRACT-V1.md', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-history-*.log'))
for name in ['p4-workbuddy-history-runtime.json', 'workbuddy-history-restart-20261010.json', 'workbuddy-history-window-20261010.png']:
    paths.add(reports / name)
result = {'checkpoint': 55, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'realReadOnlyHistory': True, 'liveRounds': 4, 'repeatedObservationMatches': True, 'modelIdentityAuthenticated': False, 'externalUserMessages': 0, 'realModelCalls': 0, 'parserCases': 19, 'nativeComposerCases': 19, 'fullRust': {'passed': 417, 'failed': 0, 'ignored': 94}, 'nodeTests': 5, 'jobs': jobs, 'runtime': runtime, 'previousIndexVerifiedCounts': previous, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected files only; not a reproducible full build', 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 55, 'records': len(result['records']), 'jobs': len(jobs), 'goal': 'active'}))
