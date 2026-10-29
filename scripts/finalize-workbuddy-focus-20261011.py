"""Freeze checkpoint 060 once after focus fix checks and approved live attempt."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-focus-review-20261011.json'
archive = reports / 'p4-workbuddy-focus-sources-20261011'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

prior_name = 'reports/p4-workbuddy-send-review-20261011.json'
prior = load(prior_name)
previous = {}
for name in [*prior['previousIndexVerifiedCounts'], prior_name]:
    old = load(name)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[name] = len(old['records'])
jobs, directories, results = {}, [], {}
for directory in sorted(reports.glob('workbuddy-focus*owned-20261011*')):
    job = load(directory / 'cleanup.json')
    assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0
    jobs[directory.name] = job
    output = json.loads(log(directory / 'stdout.log').strip())
    native = Path(output['report'])
    assert native.is_relative_to(reports)
    directories.extend([directory, native])
    results[directory.name] = load(native / 'result.json')
native = results['workbuddy-focus-send-owned-20261011-02']
assert native['status'] == 'passed' and native['unmodifiedProductionObserver'] and native['rustContractPassed']
assert len(native['cases']) == 26 and all(c['passed'] for c in native['cases'])
assert native['invokes'] == 4 and native['externalMessages'] == native['modelCalls'] == 0
focused = results['workbuddy-focus-owned-20261011-06']['response']['result']
assert focused['status'] == 'passed' and any(s.get('productionFocusAccepted') is True for s in focused['steps'])
live = results['workbuddy-focus-live-owned-20261011']
assert live['sendAttempts'] <= 2
checks = log('reports/p4-workbuddy-focus-check-20261011.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (425, 0, 96)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
for kind in ['clippy', 'desktop-build']:
    assert 'Finished' in log(f'reports/p4-workbuddy-focus-{kind}-20261011.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({r['sha256'] for r in runtime}) == 1
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
live_note = ('重新批准的两条真实标记消息发送及回复凭据通过，末次连续准备通过。TaskReview/attempt仍为诊断合成，未经过持久批准/派发/独立验收。' if live['status'] == 'passed' else f'重新批准的实机验证未通过；本次发送尝试{live["sendAttempts"]}次，错误：{live.get("error", "见报告").splitlines()[0]}。失败即停，不自动重发，不断言零副作用。')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert '## 060 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 060 WorkBuddy实机焦点定位与修复 {stamp}

本轮progress，目标active。本日/本卷第005项，完整P0–P6未验收。

独立无消息诊断复现WorkBuddy5.7.7输入框SetFocus抛异常：控件enabled但IsKeyboardFocusable=false，原前台/历史/按钮验证通过。加入一次受保护点击聚焦：核对前台身份、修饰键、目标矩形、顶层遮挡及精确UIA命中，点击后仍须完整观察匹配原审阅。首次聚焦变化不会被自动接受，需要重新准备。输入框已获可靠焦点则不再点击。

实机全局焦点来自宿主Document，内容提供器的唯一Edit才标记HasKeyboardFocus。适配仅限5.7.7、动态原hwnd对应42.{{hwnd}}.4.3文档、同进程且直接父Window为原hwnd、唯一获焦可见Edit为原输入框。实机生产焦点检查无键盘输入通过，仍保留原精确对象匹配分支。详见docs/WORKBUDDY-FOCUS-2026-10-11.md。

发送stderr新增固定阶段枚举，不包含聊天/草稿/账号内容，JSON协议不变；实机脚本保存stderr。attempt阶段不证明副作用完成。26个受控UIA/键盘场景通过，含不可SetFocus的点击路径、错误命中和聚焦后草稿变化的输入前拒绝，4次受控Invoke；跨语言回复契约通过。受控证据不替代实机任务。

{live_note}
阶段日志确认到keyboard-attempt、invoke-attempt和read-reply。事后只读核查reports/workbuddy-draft-diagnostic-13nJOK显示87字符草稿与原批准消息完全一致、原4轮历史摘要不变、提示词不在历史、会话空闲。发送按钮Invoke返回但未获得提交证据；第二条未尝试，草稿保留。下一步需实现有界真实点击发送适配，并修正以循环次数充当等待秒数的问题；不能重输原消息。任何进一步提交残留草稿需先给出具体操作范围再确认。实机报告与Job清理记录均收录本批索引；不得把短文本探针当成编程交付或取消验收。

425默认Rust/0失败/96忽略、5Node、fmt、严格clippy和前端/核心/桌面调试构建通过，三runtime一致。所有本批Job均回收后0，041–059冻结证据有效。无HEAD/推送/云CI。

后续：按实机结果推进或修复真实发送，完善丰富代码/工具回复、真实取消、宿主/UI/调度及持续编程任务。真实OpenAI主副指挥、全部首批API/CLI、恢复专项、P5异构并行和P6干净安装/升级/回滚/迁移/签名范围保持。不要重跑finalize。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第005项（批次060）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [焦点修复](WORKBUDDY-FOCUS-2026-10-11.md) · [本批证据](../reports/p4-workbuddy-focus-review-20261011.json)

已复现WorkBuddy SetFocus不支持问题，增加受保护点击及同窗口宿主Document/唯一获焦Edit核对；不放宽原观察变化拒绝。实机无键盘输入检查通过，26受控发送场景及跨语言契约通过。

{live_note}

事后只读核对：87字符草稿仍完全匹配批准消息，原4轮历史未变，空闲且提示词不在历史。草稿保持，未重输重发；需适配真实发送点击并重新确认提交范围。

425默认Rust/96忽略、5Node、fmt/严格clippy、完整调试构建通过，三runtime一致；本批测试Job回收后0，041–059旧索引有效。发送stderr保留固定阶段，帮助区分输入/发送/回读故障，不记录正文。

真实编程/取消、宿主WorkBuddy持久派发、丰富回复、UI/调度与完整P4/P5/P6仍未验收，真实OpenAI管理、其余API/CLI与恢复范围保持。目标active，本日/本卷5项，无HEAD/推送/云CI；不要重跑finalize。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-11.md'
matrix.write_text(matrix.read_text(encoding='utf-8').rstrip() + f'\n\n{stamp} 本日005（060）：实机焦点故障复现修复，26受控场景和425默认Rust/96忽略、5Node及完整构建检查通过。{live_note}完整P4/P5/P6仍未验收。\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'tests/fixtures/WorkBuddyStopWindow.cs', 'tests/workbuddy-send-native.mjs', 'tests/workbuddy-focus-diagnostic.mjs', 'tests/workbuddy-send-live.mjs', 'tests/workbuddy-send-reconcile.mjs', 'tests/workbuddy-draft-diagnostic.mjs', 'tests/fixtures/workbuddy-live-plan.json', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-focus-20261011.py', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md', 'docs/WORKBUDDY-FOCUS-2026-10-11.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-focus-*.log'))
record = {'checkpoint': 60, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'nativeCases': 26, 'nativeInvokes': 4, 'liveResult': live, 'durableWorkBuddyDispatchTested': False, 'fullRust': {'passed': 425, 'failed': 0, 'ignored': 96}, 'nodeTests': 5, 'jobs': jobs, 'runtime': runtime, 'previousIndexVerifiedCounts': previous, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected files only; not a full reproducible build', 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 60, 'records': len(record['records']), 'goal': 'active'}))
