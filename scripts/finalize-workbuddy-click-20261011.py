"""Freeze checkpoint 061 once after guarded click and elapsed-wait verification."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-click-review-20261011.json'
archive = reports / 'p4-workbuddy-click-sources-20261011'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    data = (root / p).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

prior_name = 'reports/p4-workbuddy-focus-review-20261011.json'
prior = load(prior_name)
previous = {}
for name in [*prior['previousIndexVerifiedCounts'], prior_name]:
    old = load(name)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[name] = len(old['records'])
jobs, directories, results = {}, [], {}
for directory in sorted(reports.glob('workbuddy-click*owned-20261011*')):
    job = load(directory / 'cleanup.json')
    assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0
    jobs[directory.name] = job
    output = json.loads(log(directory / 'stdout.log').strip())
    native = Path(output['report'])
    assert native.is_relative_to(reports)
    directories.extend([directory, native])
    results[directory.name] = load(native / 'result.json')
native = results['workbuddy-click-send-owned-20261011-02']
assert native['status'] == 'passed' and native['unmodifiedProductionObserver'] and native['rustContractPassed']
assert len(native['cases']) == 30 and all(c['passed'] for c in native['cases'])
assert native['sendClicks'] == 5 and native['invokes'] == native['externalMessages'] == native['modelCalls'] == 0
draft = results['workbuddy-click-draft-native-owned-20261011']
assert draft['status'] == 'passed' and draft['keyboardCharacters'] == draft['invokes'] == 0 and draft['sendClicks'] == 1 and draft['rustContractPassed']
assert results['workbuddy-click-stop-owned-20261011']['status'] == 'passed'
live = results['workbuddy-click-draft-preflight-owned-20261011']
assert live['status'] == 'failed' and live['clickAttempts'] == 0
state = load('reports/p4-workbuddy-click-live-state-20261011.json')
assert not state['originalProcessPresent'] and state['installedAppProcessCount'] == 0
checks = log('reports/p4-workbuddy-click-check-20261011.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (425, 0, 96)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
for kind in ['clippy', 'desktop-build']:
    assert 'Finished' in log(f'reports/p4-workbuddy-click-{kind}-20261011.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({r['sha256'] for r in runtime}) == 1
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert '## 061 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 061 WorkBuddy真实点击发送适配与按耗时等待 {stamp}

本轮progress，目标active。本日/本卷第006项。完整P0–P6仍未验收。

生产发送路径现对WorkBuddy只使用一次真实点击，Qoder保持Invoke。不是Invoke后的补点或未知重试。点击前重新读取原文档/历史/模型/完整草稿，唯一输入区发送按钮必须匹配原RuntimeId，核对原前台窗口/进程、修饰键、按钮矩形、顶层遮挡和UIA命中（允许按钮内部子节点但须沿父链到原按钮），最后再次核对可见启用状态、矩形和当前草稿。SendInput仅一次鼠标down/up，部分发送失败即返回未知，不补偿点击。

回复等待改为Stopwatch单调时钟，包括UIA读取耗时。到时不开始新一轮读取，仍保留宿主外层进程超时以回收阻塞COM；不声称每个UIA调用均可被内层严格中断。受控慢读1200毫秒/预算1秒只启动一次读取，替代原maxWaitSeconds*2次循环累计超时。

最终受控跨进程UIA/真实键盘及点击30场景通过、5次发送点击/0Invoke，包含连续任务、聚焦保护、发送命中错误、未知不重发和慢读计时。停止55场景回归通过。专用既有草稿提交工具的受控场景通过：0键盘字符/0Invoke/1次点击，过期或变化观察、被用户更改草稿、模型变化及成功后重复点击均拒绝；实际返回经Rust生产回复验证。工具只是测试事件处置，未开放产品恢复接口。

真实草稿预检未通过：原PID3616已退出，20:37核对安装路径D:/workbuddy/WorkBuddy.exe当前无进程。工具未点击、未输入、未自动重绑或重启；本轮真实消息尝试0。060的87字符原草稿在进程退出后的持久状态未知，不能声称仍存在或已丢失。固定原窗口的草稿提交计划现在不可执行，重新启动后必须先只读核对，再创建绑定新身份的新审阅并明确确认；禁止改旧报告或复用旧窗口身份。

专用测试计划tests/fixtures/workbuddy-draft-submit-plan.json绑定原TaskReview及prompt SHA，默认只读。授权时以独占创建并flush的计划摘要回执限制一次点击尝试，不重输、不发送第二条。当前计划失效，不应直接授权运行。报告和失败调查均保留。

425默认Rust/0失败/96忽略、5Node、fmt/严格clippy及前端/核心/桌面debug构建通过，三runtime一致。本批Job全部回收后0，041–060旧冻结证据有效；选择性源码归档不等于完整可复现构建。无HEAD/推送/云CI。

后续真实WorkBuddy需要重新发现和确认现有草稿处置；本地可继续丰富代码/工具回复、宿主/UI/调度及持久连续任务接入。真实编程交付/取消仍未验收。真实OpenAI管理、全部首批API/CLI、恢复专项、P5异构worktree和P6干净安装/升级/回滚/迁移/签名范围保持。不要重跑finalize。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第006项（批次061）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-click-review-20261011.json)

WorkBuddy发送改为一次受保护真实点击，Qoder保持Invoke；不是错误后的补点。回复等待按单调时钟计实际耗时。30受控场景/5发送点击/0Invoke，慢读预算与跨语言凭据通过；专用草稿提交受控0键盘/1点击通过，停止55回归通过。

真实草稿预检拒绝：原PID3616已退出，20:37安装路径无进程。本轮0真实点击/输入/消息尝试。原87字符草稿是否持久保留未知；旧草稿计划绑定已失效窗口，不可直接授权运行。需重新发现、只读核对并创建新身份审阅后确认，不能复用旧记录。

425默认Rust/96忽略、5Node、fmt/严格clippy和完整debug构建通过，三runtime一致；本批Job回收后0，041–060旧索引有效。真实发送/取消/编程、WorkBuddy宿主派发、丰富回复及完整P4/P5/P6仍未验收，真实OpenAI管理、其余API/CLI与恢复范围保持。目标active，本日/本卷6项，无HEAD/推送/云CI，不要重跑finalize。
''', encoding='utf-8')
for name, text in {
    'docs/STAGE-ACCEPTANCE-2026-10-11.md': f'{stamp} 本日006（061）：WorkBuddy一次真实点击发送、按耗时等待实现，30受控场景及专用草稿提交通过，停止55回归和425默认Rust/96忽略、5Node、完整构建通过。真实预检因原进程已退出拒绝，未点击；完整P4/P5/P6仍未验收。',
    'docs/WORKBUDDY-ACTION-CONTRACT-V1.md': '## 061 发送动作改为一次真实点击\n\nWorkBuddy5.7.7发送按钮Invoke曾返回但草稿未提交，因此生产发送路径改为一次受保护物理点击，绝不先Invoke再补点。原文档/模型/历史/完整草稿、唯一输入区按钮、前台/覆盖窗口、UIA命中与按钮当前状态必须一致。未知不重发。Qoder仍用Invoke。回复等待使用Stopwatch包含UIA耗时；阻塞调用由外层进程时限回收。\n\n30项受控UIA场景及专用草稿提交测试通过；专用工具不向生产协议暴露恢复操作，不等于产品持久派发。真实原进程已退出，旧草稿计划失效，本轮没有实机发送，后续应重新核查并建立新的审阅。见061交接。'
}.items():
    path = root / name
    path.write_text(path.read_text(encoding='utf-8').rstrip() + '\n\n' + text + '\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'crates/core/native/DesktopTrial.cs', 'crates/core/native/DesktopStop.cs', 'tests/fixtures/WorkBuddyStopWindow.cs', 'tests/fixtures/WorkBuddyDraftProbe.cs', 'tests/fixtures/workbuddy-draft-submit-plan.json', 'tests/workbuddy-send-native.mjs', 'tests/workbuddy-draft-native.mjs', 'tests/workbuddy-draft-submit.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-click-20261011.py', 'docs/HANDOFF-LATEST.md', 'docs/STAGE-ACCEPTANCE-2026-10-11.md', 'docs/WORKBUDDY-ACTION-CONTRACT-V1.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-click-*.log'))
paths.add(reports / 'p4-workbuddy-click-live-state-20261011.json')
record = {'checkpoint': 61, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'nativeCases': 30, 'nativeSendClicks': 5, 'nativeDraftCases': len(draft['cases']), 'externalSendAttempts': 0, 'liveState': state, 'durableWorkBuddyDispatchTested': False, 'fullRust': {'passed': 425, 'failed': 0, 'ignored': 96}, 'nodeTests': 5, 'jobs': jobs, 'runtime': runtime, 'previousIndexVerifiedCounts': previous, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected files only; not a full reproducible build', 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 61, 'records': len(record['records']), 'goal': 'active'}))
