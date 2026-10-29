"""Freeze checkpoint 059 once; native synthetic tasks are not product acceptance."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-send-review-20261011.json'
archive = reports / 'p4-workbuddy-send-sources-20261011'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

prior_name = 'reports/p4-workbuddy-stop-action-review-20261011.json'
prior = load(prior_name)
previous = {}
for name in [*prior['previousIndexVerifiedCounts'], prior_name]:
    old = load(name)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[name] = len(old['records'])
jobs, directories, results = {}, [], {}
for directory in sorted(reports.glob('workbuddy-send-*-owned-20261011*')):
    job = load(directory / 'cleanup.json')
    assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0
    jobs[directory.name] = job
    output = json.loads(log(directory / 'stdout.log').strip())
    native_dir = Path(output['report'])
    assert native_dir.is_relative_to(reports)
    directories.extend([directory, native_dir])
    if (native_dir / 'result.json').exists():
        results[directory.name] = load(native_dir / 'result.json')
native = results['workbuddy-send-native-owned-20261011-03']
assert native['status'] == 'passed' and native['realUia'] and native['unmodifiedProductionObserver']
assert native['rustContractPassed'] and native['keyboardInputTested']
assert len(native['cases']) == 20 and all(c['passed'] for c in native['cases'])
assert native['invokes'] == 3 and native['valueChanges'] == native['externalMessages'] == native['modelCalls'] == 0
for kind, count in [('stop', 55), ('reply', 21)]:
    result = results[f'workbuddy-send-{kind}-owned-20261011']
    assert result['status'] == 'passed' and len(result['cases']) == count
for name in ['workbuddy-send-native-owned-20261011-03', 'workbuddy-send-stop-owned-20261011', 'workbuddy-send-reply-owned-20261011']:
    assert jobs[name]['ExitCode'] == 0 and not jobs[name]['TimedOut'] and not jobs[name]['Error']
checks = log('reports/p4-workbuddy-send-check-20261011.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (425, 0, 96)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
for kind in ['clippy', 'desktop-build']:
    assert 'Finished' in log(f'reports/p4-workbuddy-send-{kind}-20261011.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({r['sha256'] for r in runtime}) == 1
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert '## 059 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 059 WorkBuddy原生发送与连续任务 {stamp}

本轮progress，目标active。本日/本卷第004项。完整P0–P6未验收；承接058。本轮新增的实机脚本默认只读，执行需匹配具体消息计划摘要。19:49只读发现WorkBuddy已退出，0发送；随后按已有授权重新启动用于只读预检。重新导航成功。用户明确批准最多两条计划消息后，实机预检4轮历史通过。首条发送尝试返回observation_unavailable，第二条未尝试；不能仅据这个错误判断有无副作用。事后只读核对reports/workbuddy-send-reconcile-mRS39z：原4轮历史摘要未变、空草稿、空闲、没有对应回复，未证明发送成功。未以此断言服务端绝无请求。

生产C# tasksend增加WorkBuddy5.7.7分支，复用审阅/时间/互斥/原进程窗口/文档模型历史/空草稿/焦点核对与Unicode单次输入；输入前后都核对唯一版本化输入区工具栏发送按钮，其他区域按钮不可替代。输入后须全文一致、原历史和模型不变，再Invoke一次。此后仅只读查结果，未知不重输、不重发、不清除残留草稿。原生桥接无持久去重；跨调用一次性资格仍属Rust宿主。ReviewTask的参数改名allowWorkBuddy以免将动作分支误称只读。

WorkBuddy taskprepare现在允许前序TaskReview：核对原窗口/文档/模型、历史后缀和原提示词/终态/末尾标记后才返回下一轮观察与历史。原sessionId保持null。产品Rust宿主的WorkBuddy派发门槛保持，原生准备不代替持久前序记录和用户批准。

最终受控UIA测试reports/workbuddy-send-native-1xCirr/result.json通过20场景，生产观察器未替换。真实SendInput输入中文、引号和emoji，实际Invoke共3次：同一聊天连续两项任务各一次，另一个无确认场景一次。连续历史3→4→5轮；过期/摘要不符/重复按钮在输入前拒绝，输入后模型变化/按钮禁用不发送，成功后的旧审阅不能重发，未知仅只读回查。第一项实际UIA回复交Rust生产验证与序列化回查通过；TaskReview规范摘要由Rust计算，但请求与尝试为合成，未经过批准/持久派发/独立验收。

早期失败是测试窗口逐个UTF-16代码单元写UTF-8日志，遇emoji高代理字符时崩溃；通用observation_unavailable来自已退出测试进程。诊断确认键盘已进入测试窗口，不能将valueChanges=0解释为键盘输入0。修复为等待低代理字符再写日志；未经修改的生产观察器最终通过。失败和带详细异常的诊断报告保留，不能计入产品验收。受控fixture独立记录键盘字符与Invoke，ValuePattern写入仍为0。

停止55场景、回复21场景与跨语言契约回归通过。425默认Rust/0失败/96忽略、5Node、fmt、严格clippy、前端/核心/桌面调试构建通过，三runtime一致。所有本批Job包含失败调查均回收后0；041–058冻结索引有效。无HEAD/推送/云CI。

下一步：在明确的实机消息验证范围内验证真实输入/回复/取消与丰富代码/工具结构，接入宿主本地binding、连续任务持久记录、界面审阅与调度，再做实际编程交付。用户随后已明确批准计划中的两条消息。首次真实发送尝试报observation_unavailable，按约定失败即停，第二条未尝试，不自动重发。后续应先查明失败阶段，修复后重新确认是否重试。真实OpenAI主副指挥、其余恢复专项、全部首批API/CLI、P5异构worktree和P6安装/升级/回滚/迁移/签名验收范围保持。不要重跑本finalize。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第004项（批次059）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-send-review-20261011.json)

WorkBuddy原生tasksend及带前序taskprepare已实现，受控真实UIA/键盘20场景通过，包含连续两项任务、历史3→5轮和异常拒绝。3次Invoke含1次无确认场景；未知只读回查，不重输重发。原生桥接不提供持久去重，产品宿主WorkBuddy派发仍有门槛。

本轮用户批准最多两条真实标记消息。首条发送尝试返回observation_unavailable，脚本停止，第二条未尝试；实际消息数/模型调用不作0或成功声明。只读回查显示原4轮历史未变、空草稿空闲、无对应回复。本轮合成尝试与UIA回复契约不替代真实发送/取消/编程、持久批准/派发与独立验收。批准计划与首条失败记录已保存；失败后不自动重发。

425默认Rust/96忽略、5Node、fmt/严格clippy、完整调试构建及三runtime一致；停止55与回复21回归通过。所有Job含失败调查回收后0，041–058旧索引有效。首次emoji日志故障属于fixture，诊断和失败报告保留。

后续推进真实WorkBuddy运行/回复/取消、丰富编程结果结构、宿主/UI/调度与持续任务；完整P4/P5/P6、真实OpenAI管理和全部首批API/CLI仍待验收，目标active。本日/本卷4项，无HEAD/推送/云CI，不要重跑finalize。
''', encoding='utf-8')
updates = {
    'docs/STAGE-ACCEPTANCE-2026-10-11.md': f'{stamp} 本日004（059）：WorkBuddy原生发送与连续准备实现，受控UIA/真实键盘20场景及连续两任务通过；停止55/回复21回归通过，425默认Rust/96忽略、5Node和完整构建检查通过。真实首条发送尝试报观察错误，未重试，第二条未尝试；持久宿主派发、真实发送/取消/编程与完整P4/P5/P6仍未验收。',
    'docs/WORKBUDDY-ACTION-CONTRACT-V1.md': '## 059 原生发送与连续准备\n\nWorkBuddy5.7.7的tasksend现可使用原任务审阅，先核对原观察/历史/模型/空草稿与唯一输入区发送按钮，再输入全文、复核内容与按钮后Invoke一次。Unicode键盘路径复用Qoder，ValuePattern只读核对；不使用剪贴板，不按Enter，不重输、不重发，未知只读查询。输入后异常保留草稿。taskprepare带前序TaskReview时，必须证明上一项任务的原提示词、历史后缀、完成状态与标记，才能准备下一项。\n\n20项受控UIA/真实键盘场景通过，连续两项任务使历史3→5轮。合成尝试经规范摘要和C#→Rust回复验证，不证明持久派发/独立验收。产品宿主WorkBuddy版本门槛仍保留；真实发送/取消与丰富编程输出仍待验证。本轮经用户批准进行首条真实发送尝试，但返回观察错误，随即停止；不声称发送成功或零副作用。参考059交接和reports/p4-workbuddy-send-review-20261011.json。'
}
for name, content in updates.items():
    path = root / name
    path.write_text(path.read_text(encoding='utf-8').rstrip() + '\n\n' + content + '\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'tests/fixtures/WorkBuddyStopWindow.cs', 'tests/workbuddy-send-native.mjs', 'tests/workbuddy-send-live.mjs', 'tests/workbuddy-send-reconcile.mjs', 'tests/fixtures/workbuddy-live-plan.json', 'tests/workbuddy-stop-native.mjs', 'tests/workbuddy-reply.mjs', 'tests/workbuddy-history.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-send-20261011.py', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md', 'docs/WORKBUDDY-ACTION-CONTRACT-V1.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-send-*.log'))
record = {'checkpoint': 59, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'nativeScope': 'owned UIA/keyboard fixture; synthetic review, not product dispatch', 'nativeCases': 20, 'nativeInvokes': 3, 'externalMessages': None, 'realModelCalls': None, 'liveSendAttempts': 1, 'liveResult': 'observation_unavailable; stopped without retry', 'durableWorkBuddyDispatchTested': False, 'fullRust': {'passed': 425, 'failed': 0, 'ignored': 96}, 'nodeTests': 5, 'jobs': jobs, 'runtime': runtime, 'previousIndexVerifiedCounts': previous, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected files only; not a full reproducible build', 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 59, 'records': len(record['records']), 'goal': 'active'}))
