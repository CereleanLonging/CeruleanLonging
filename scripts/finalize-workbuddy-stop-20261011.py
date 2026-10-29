"""Freeze checkpoint 057 once after bounded stop observation verification."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-stop-review-20261011.json'
archive = reports / 'p4-workbuddy-stop-sources-20261011'
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
old_paths += [f'reports/p4-workbuddy-{label}-review-20261010.json' for label in ['composer', 'model', 'history', 'proof']]
for path in old_paths:
    old = load(path)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[path] = len(old['records'])

jobs, directories, successful_native, regression = {}, [], [], {}
for directory in sorted(reports.glob('workbuddy-stop-*-owned-20261011*')):
    job = load(directory / 'cleanup.json')
    assert job['CleanupConfirmed'] and job['RemainingProcesses'] == 0, directory
    jobs[directory.name] = job
    directories.append(directory)
    if job['ExitCode'] == 0:
        native = Path(json.loads(log(directory / 'stdout.log').strip())['report'])
        assert native.is_relative_to(reports)
        directories.append(native)
        if directory.name.startswith('workbuddy-stop-native-owned'):
            successful_native.append(native)
        elif directory.name in ['workbuddy-stop-qoder-owned-20261011','workbuddy-stop-reply-owned-20261011']:
            regression[directory.name] = load(native / 'result.json')
assert successful_native
native = successful_native[-1]
result = load(native / 'result.json')
assert result['status'] == 'passed' and result['realUia'] and result['rustContractPassed']
assert result['invokes'] == result['valueChanges'] == result['messagesSent'] == result['modelCalls'] == 0
assert all(c['passed'] for c in result['cases'])
assert regression['workbuddy-stop-qoder-owned-20261011']['status'] == 'passed'
assert len(regression['workbuddy-stop-qoder-owned-20261011']['cases']) == 17
assert regression['workbuddy-stop-reply-owned-20261011']['status'] == 'passed'
assert len(regression['workbuddy-stop-reply-owned-20261011']['cases']) == 21
# Keep failed investigations as evidence as well as the passing run.
directories.extend(p for p in reports.glob('workbuddy-stop-native-*') if p.is_dir() and p not in directories)
checks = log('reports/p4-workbuddy-stop-check-20261011.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and tuple(sum(c[i] for c in counts) for i in range(3)) == (425, 0, 96)
assert re.search(r'pass 2\b', checks) and re.search(r'pass 3\b', checks)
assert 'Finished' in log('reports/p4-workbuddy-stop-clippy-20261011.log')
assert 'Finished' in log('reports/p4-workbuddy-stop-desktop-build-20261011.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({v['sha256'] for v in runtime}) == 1
(reports / 'p4-workbuddy-stop-runtime-20261011.json').write_text(json.dumps(runtime, indent=2) + '\n', encoding='utf-8')
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%Y-%m-%d %H:%M +0800')
daily = root / 'handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md'
assert '## 057 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 057 WorkBuddy停止只读核对与原生UIA证据 {stamp}

本轮progress，目标active。本日/本卷第002项，承接056；完整P0–P6未通过。

生产C#新增WorkBuddy5.7.7停止历史解析：保留至少两轮完整历史后缀，只核对最后配对中的原用户提示词、助手角色与独立状态；当前取消回复可以为空。停止控件必须位于同一个输入区的实际工具栏结构，历史正文/其他停止控件不能替代。重复候选、其他停止任务按钮、草稿/模型/文档变化、历史缺口和异常终态前缀均拒绝。两次读取必须给出一致精确取消终态、原turn/prompt、历史后缀且已无停止按钮。taskstopprepare与taskstopread只读分支开放，实际taskstop、发送和产品派发仍有门槛。

Rust取消凭据按应用核对状态。WorkBuddy需原TaskReview摘要仍正确，该摘要绑定原本地binding/尝试/观察；禁止供应商sessionId，精确支持五种取消标签，不接受额外文字或空白。Qoder原格式保持。6个默认测试覆盖身份、原审阅、历史、时间与状态变化；不存在额外伪造会话ID。

真实跨进程UIA受控测试窗口通过{len(result['cases'])}场景，见{native.relative_to(root).as_posix()}；它是自建WPF HWND/原始UIA树，生产观察器未替换，不是实际WorkBuddy任务。Rust计算规范TaskReview摘要后交给C#，实际停止观察/取消凭据再交回Rust校验与序列化回查通过；请求/意图为合成，未经过真实批准/持久取消流程。零Invoke、零输入、零模型/外部消息。首次测试的command.json并发读写锁错误已改为按序号独立发布交接文件，失败证据保留。

10月11日07:14实机只读发现仍存在窗口，但WorkBuddy无可识别输入区、Qoder无唯一Document；没有输入、导航、停止或重启用户应用。受控测试不提升实机取消/发送/编程验收状态。真实运行/取消结构、丰富代码/工具回复解析、宿主/UI/调度及持久恢复仍需继续。

425默认Rust/0失败/96忽略、5Node、fmt、完整前端/核心/桌面debug构建及全workspace/all-targets/custom-protocol严格clippy通过；三runtime一致。原有Qoder受控停止17场景及WorkBuddy合成回复21场景/跨语言契约回归通过。所有本批测试Job回收后0，失败调查独立保留。041–056旧索引校验通过，选择性源码与证据见../../reports/p4-workbuddy-stop-review-20261011.json。无HEAD/推送/云CI。

下一步实现受控单次发送/停止动作并复验真实WorkBuddy结构，完善丰富编程回复和连续任务，再接宿主/UI/调度端到端。真实OpenAI主副指挥、其余恢复专项、全部首批API/CLI、P5异构并行及P6安装/升级/回滚/迁移/签名全范围不变。不要重跑本finalize。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-11 卷001第002项（批次057）{stamp}](../handoffs/2026-10-11/handoff_2026-10-11_001_070900+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-11.md) · [WorkBuddy动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-stop-review-20261011.json)

WorkBuddy原任务停止准备/取消回读已接只读分支：原历史、提示词、输入区停止控件及两次精确取消终态核对。Rust核对原审阅摘要和本地关联，sessionId保持null。受控UIA {len(result['cases'])}场景及实际C#→Rust停止凭据契约通过，零输入/Invoke；尚非真实应用取消、持久派发或编程验收。

425默认Rust/96忽略、5Node、fmt/严格clippy及前端/核心/桌面构建通过，三runtime一致；本批Job含失败调查均回收后0。041–056旧索引有效。07:14实机控件不可用，未操作用户窗口；后续操作前重新发现。

下一步：单次发送/停止动作、真实运行/取消与代码/工具回复结构，再接宿主/UI/调度/连续任务。完整P4及真实OpenAI管理、其余恢复、全部首批API/CLI、P5/P6仍未验收，目标active。本日/本卷2项，无HEAD/推送/云CI；不要重跑finalize。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-11.md'
matrix.write_text(matrix.read_text(encoding='utf-8').rstrip() + f'\n\n{stamp} 本日002（057）：WorkBuddy停止只读关联与取消核对实现，受控UIA {len(result["cases"])}场景和跨语言契约通过；实际按钮未启用，实机取消/编程仍未验收。425默认Rust/96忽略、5Node及完整检查构建通过。完整P4/P5/P6状态不变，见057交接。\n', encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs','crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs','crates/core/src/desktop_cancellation.rs','crates/core/src/desktop_workbuddy_cancel_tests.rs','tests/fixtures/WorkBuddyStopWindow.cs','tests/workbuddy-stop-native.mjs','tests/workbuddy-reply.mjs','scripts/test-omp-stop-review-native.ps1','scripts/finalize-workbuddy-stop-20261011.py','docs/WORKBUDDY-ACTION-CONTRACT-V1.md','docs/HANDOFF-LATEST.md','docs/STAGE-ACCEPTANCE-2026-10-11.md',daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file())
paths.update(reports.glob('p4-workbuddy-stop-*.log'))
paths.add(reports / 'p4-workbuddy-stop-runtime-20261011.json')
record = {'checkpoint':57,'createdAt':datetime.now(timezone.utc).isoformat(),'status':'partial-stage-progress','nativeScope':'owned raw UIA provider; not WorkBuddy cancellation','nativeCases':len(result['cases']),'externalMessages':0,'realModelCalls':0,'fullRust':{'passed':425,'failed':0,'ignored':96},'nodeTests':5,'jobs':jobs,'runtime':runtime,'previousIndexVerifiedCounts':previous,'p4':'incomplete','p6':'incomplete','sourceArchive':'selected files only; not a full reproducible build','records':[{'path':p.relative_to(root).as_posix(),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint':57,'records':len(record['records']),'goal':'active'}))
