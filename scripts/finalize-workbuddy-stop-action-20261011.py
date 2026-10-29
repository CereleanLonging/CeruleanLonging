"""Freeze checkpoint 058 once, preserving the distinction between fixture and live evidence."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-stop-action-review-20261011.json'
archive = reports / 'p4-workbuddy-stop-action-sources-20261011'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

previous = {}
prior = load('reports/p4-workbuddy-stop-review-20261011.json')
for path in [*prior['previousIndexVerifiedCounts'], 'reports/p4-workbuddy-stop-review-20261011.json']:
    old = load(path)
    for record in old['records']:
        assert sha(record['path']) == record['sha256'], record['path']
    previous[path] = len(old['records'])

jobs, directories, results = {}, [], {}
for directory in sorted(reports.glob('workbuddy-stop-action*owned-20261011*')):
    job = load(directory / 'cleanup.json')
    assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0, directory
    assert job['ExitCode'] == 0 and not job['TimedOut'] and not job['Error'], directory
    output = json.loads(log(directory / 'stdout.log').strip())
    native = Path(output['report'])
    assert native.is_relative_to(reports)
    jobs[directory.name] = job
    directories.extend([directory, native])
    results[directory.name] = load(native / 'result.json')

native = results['workbuddy-stop-action-owned-20261011']
assert native['status'] == 'passed' and native['realUia'] and native['rustContractPassed']
assert len(native['cases']) == 55 and all(c['passed'] for c in native['cases'])
assert native['invokes'] == native['actionScenarios'] == 4
assert native['valueChanges'] == native['messagesSent'] == native['modelCalls'] == 0
assert native['readOnlyMutations'] == {'valueChanges': 0, 'invokes': 0}
for kind, expected in [('qoder', 17), ('reply', 21)]:
    result = results[f'workbuddy-stop-action-{kind}-owned-20261011']
    assert result['status'] == 'passed' and len(result['cases']) == expected
live = results['workbuddy-stop-action-history-owned-20261011']
assert live['status'] == 'passed' and live['readOnly'] and live['liveRounds'] == 4
assert live['repeatedReadMatches'] and live['repeatedObservationMatches']
assert live['messagesSent'] == live['realModelCalls'] == 0
assert live['liveObservation']['modelLabel'] == 'Hy3'
nav = results['workbuddy-stop-action-navigate-owned-20261011']['response']['result']
assert nav['navigations'] == 1 and nav['messagesSent'] == 0 and nav['testLabels'] == 2

checks = log('reports/p4-workbuddy-stop-action-check-20261011.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (425, 0, 96)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
for kind in ['clippy', 'desktop-build']:
    assert 'Finished' in log(f'reports/p4-workbuddy-stop-action-{kind}-20261011.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({r['sha256'] for r in runtime}) == 1
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert '## 058 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 058 WorkBuddy单次停止动作与重启后只读复验 {stamp}

本轮progress，目标active。本日/本卷第003项；完整P0–P6未验收。用户授权重新启动WorkBuddy并核查原验证聊天。

原生taskstop对WorkBuddy5.7.7开放同一个一次Invoke路径：停止观察30秒有效、进程/窗口/文档/模型/历史/原提示词一致，互斥与前台复核后再次观察，只Invoke一次，之后10秒仅只读查终态。持久跨调用去重仍由Rust宿主的一次性资格负责；原生桥接本身不持久记录动作，不能声称任意重试调用均无副作用。WorkBuddy产品宿主派发门槛、tasksend和连续任务仍关闭。

未替换生产观察器的自建WPF原始UIA窗口55场景通过；四种停止操作各Invoke一次，输入修改0。成功取消、无确认、完成竞争、失败竞争分别验证；后两者不冒充取消，未知后的终态只读回查不再点击。成功后的重复请求拒绝，过期/模型/提示词/停止控件/草稿变化不触发点击。实际UIA停止返回再交Rust校验通过，但请求/意图仍合成，未经过持久WorkBuddy批准/派发/取消链。

实机首先确认WorkBuddy已退出，然后启动D:/workbuddy/WorkBuddy.exe（PID16652），核对路径和唯一主窗口后恢复显示。加载初期无输入区，加载完成后首页可读；19:29对既有“工作台接入验证”行单击一次并确认内容标题。随后生产桥接连续两次只读准备通过，4轮历史摘要与完整观察一致，Hy3、空闲空草稿、sessionId=null。报告reports/workbuddy-history-j0E6mo/result.json；未读Document.Value、未输入或发送消息，未改变模型。Qoder最末观察window_not_visible，没有操作它。此为重启后真实历史复验，真实运行/取消/编程交付尚未验收。

425默认Rust/0失败/96忽略、5Node、fmt、严格clippy和前端/核心/桌面调试构建通过，三runtime摘要相同。Qoder受控停止17场景与WorkBuddy合成回复21场景/跨语言契约回归通过。所有本批Job回收后0；应用启动在测试Job之外，保留用户WorkBuddy运行。041–057冻结索引校验有效。

下一步：WorkBuddy受控单次发送/连续任务、丰富代码/工具回复结构与真实运行/取消，再接宿主/UI/调度和实际编程交付。真实OpenAI主副指挥、其余恢复、全部首批API/CLI、P5异构worktree、P6干净安装/升级/回滚/迁移/签名缺口保持。无HEAD/推送/云CI；不要重跑本finalize。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第003项（批次058）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-stop-action-review-20261011.json)

WorkBuddy原生单次停止路径55个受控UIA场景通过，4次Invoke/0输入；未知只读回查。持久去重由宿主负责，WorkBuddy产品派发/发送/连续任务仍未开放。真实取消与编程交付未验收。

已按用户授权重新启动WorkBuddy并进入原“工作台接入验证”聊天。19:29后生产桥接两次只读准备匹配4轮历史和完整观察，Hy3、空闲无草稿、sessionId=null，0真实消息/模型调用。Qoder未操作。后续动作前必须重新发现，不能复用旧PID/观察。

425默认Rust/96忽略、5Node、fmt/严格clippy、完整调试构建及三runtime一致；Qoder停止17与WorkBuddy回复21回归通过。测试Job回收后0，用户应用保留。041–057旧索引有效。

后续：单次发送/连续任务、代码/工具回复结构、真实运行/取消、宿主/UI/调度及编程交付；完整P4/P5/P6和真实OpenAI管理、全部首批API/CLI仍待验收，目标active。本日/本卷3项；无HEAD/推送/云CI，不要重跑finalize。
''', encoding='utf-8')
for name, text in [
    ('docs/STAGE-ACCEPTANCE-2026-10-11.md', f'{stamp} 本日003（058）：WorkBuddy原生单次停止路径受控UIA 55场景/4Invoke通过，真实重启后原验证聊天两次只读历史4轮一致；未发送实际消息，真实取消/编程、宿主派发及完整P4/P5/P6仍未验收。425默认Rust/96忽略、5Node及完整检查构建通过。'),
    ('docs/WORKBUDDY-ACTION-CONTRACT-V1.md', '## 058 原生单次停止动作\n\n原生taskstop现开放WorkBuddy版本化分支，沿用30秒新鲜观察、互斥、原窗口/前台/历史/模型/提示词复核和一次Invoke，后续仅只读等待。55项真实受控UIA场景通过，4次停止各Invoke一次、输入0；完成/失败竞争不当作取消。未知结果后只读回查，不自动重试点击。持久跨调用去重仍属于Rust宿主责任，原生桥接没有持久幂等记录。产品宿主WorkBuddy派发、tasksend和连续任务门槛保持。\n\n本批实机重新启动并导航原验证聊天，4轮历史连续两次匹配、Hy3、无草稿空闲，0真实消息；不能替代真实取消/发送/编程交付证据。见058交接和reports/p4-workbuddy-stop-action-review-20261011.json。'),
]:
    path = root / name
    path.write_text(path.read_text(encoding='utf-8').rstrip() + '\n\n' + text + '\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'crates/core/src/desktop_cancellation.rs', 'crates/core/src/desktop_workbuddy_cancel_tests.rs', 'tests/fixtures/WorkBuddyStopWindow.cs', 'tests/workbuddy-stop-native.mjs', 'tests/workbuddy-reply.mjs', 'tests/workbuddy-history.mjs', 'tests/workbuddy-navigate-test.mjs', 'tests/desktop-capability-diagnostic.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-stop-action-20261011.py', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md', 'docs/WORKBUDDY-ACTION-CONTRACT-V1.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-stop-action-*.log'))
record = {'checkpoint': 58, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'nativeScope': 'owned raw UIA provider; not actual WorkBuddy cancellation', 'nativeCases': 55, 'nativeInvokes': 4, 'externalMessages': 0, 'realModelCalls': 0, 'liveReadOnlyHistoryRounds': 4, 'fullRust': {'passed': 425, 'failed': 0, 'ignored': 96}, 'nodeTests': 5, 'jobs': jobs, 'runtime': runtime, 'previousIndexVerifiedCounts': previous, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected files only; not a full reproducible build', 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 58, 'records': len(record['records']), 'goal': 'active'}))
