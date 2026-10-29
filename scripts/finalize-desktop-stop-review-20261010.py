"""Freeze checkpoint 049 once, preserving all earlier evidence indexes."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-stop-review-review-20261010.json'
archive = reports / 'p4-desktop-stop-review-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    raw = (root / p).read_bytes()
    return raw.decode('utf-16' if raw.startswith(b'\xff\xfe') else 'utf-8')

old = {}
for name in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics']:
    previous = load(f'reports/p4-desktop-{name}-review-20261010.json')
    for item in previous['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    old[name] = len(previous['records'])
full = log('reports/p4-desktop-stop-review-check.log')
rows = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', full)]
counts = [sum(row[i] for row in rows) for i in range(3)]
assert counts == [417, 0, 90], counts
assert re.findall(r'(?:ℹ|#) pass (\d+)', full) == ['2', '3']
assert re.findall(r'(?:ℹ|#) fail (\d+)', full) == ['0', '0']
for name in ['clippy', 'desktop']:
    assert 'Finished' in log(f'reports/p4-desktop-stop-review-{name}.log')
assert 'test-fixtures' not in log('reports/p4-desktop-stop-review-production-features.log')
runtime = load('reports/p4-desktop-stop-review-runtime.json')
assert len(runtime) == 3 and len({r['sha256'] for r in runtime}) == 1
for item in runtime:
    assert sha(item['path']) == item['sha256']
visual = load('reports/p4-desktop-stop-review-visual-review.json')
assert visual['screenshotsReviewed'] == 11
native = load(visual['report'] + '/result.json')
assert native['status'] == 'passed' and len(native['cases']) == 3
assert {c['stage'] for c in native['cases']} == {'stop-request', 'stop-intent', 'stop-proof'}
assert all(c['originalBytesPreserved'] and c['fullCoreRestarts'] == 2 and c['unknownWindowReopen'] and c['finalState'] == 'retired' and not c['verified'] and c['dispatches'] == c['retirements'] == 1 for c in native['cases'])
regression = load(visual['regression'] + '/result.json')
assert regression['status'] == 'passed' and regression['reviewableFiles'] == 0 and regression['fullCoreRestarts'] == 2
jobs = {}
for name in ['desktop-stop-review-owned-20261010', 'desktop-stop-review-owned-20261010-02', 'desktop-stop-review-diagnostic-regression-20261010']:
    job = load(f'reports/{name}/cleanup.json')
    assert job['RemainingProcesses'] == 0 and job['CleanupConfirmed'] and not job['TimedOut']
    assert job['ExitCode'] == (1 if name == 'desktop-stop-review-owned-20261010' else 0)
    jobs[name] = job
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 049 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 049 三类桌面停止残片用户复核与本地处置 {stamp}

上一轮及本轮均有progress，目标active。完整P0–P6未通过；本日49项，本卷9项。

停止请求、停止动作意图、取消证明的空白/半写pending新增明确用户复核。要求原TaskReview、角色配置、原批准/尝试/派发事件、连续历史及有效停止前置记录相互匹配；绑定原文件清单/长度/SHA与原事件摘要。拒绝正式文件冲突、后续停止或完成证据、无效/完整JSON、改动原文、错误身份/路径和伪造复核。复核以独立create_new/sync_all/hard_link凭据保存，原文件不删除不覆盖。复核凭据自身未发布残片保留但不给恢复资格，可另存新凭据重试；已发布凭据重用而不重复复核。

新增DurableWorkflow::inspect_existing：持有原工作流锁，只读重放并校验现有记录，不执行恢复事件、不写投影、不创建目录；以不可变借用供复核使用。完整宿主重启后再次核对全部复核与原链。复核仅恢复本地管理，任务保持Unknown/verified=false；UI与核心均拒绝继续原尝试的回读/发送/停止。用户自行处理外部应用后，另行通过现有证据绑定的RetireUnknownDesktop结束本地跟踪；此后须修订并重新批准，不形成Cancelled/Submitted/Verify/Accept。停止请求残片不伪造请求正文；其他两类显示完整原请求。没有将缺失终态证明补成取消成功。

新增三项默认测试：三阶段×空白/半写的复核/双重开/本地处置；十种原文、冲突、身份和复核篡改；三阶段×五个复核发布位置的错误注入与重试（15例）。测试断言原字节/复核前后原工作流文件保持、只派发一次、无完成或取消成功事件。后者为错误注入，不是实际进程终止、断电或磁盘故障。新增一项ignored原生数据准备测试。全量417默认Rust/0失败/90忽略、5Node，fmt/严格clippy/契约/前端/核心/桌面debug构建通过；normal/build依赖无test-fixtures。三份runtime SHA：{runtime[0]['sha256']}。已有ts-rs属性提示仍存在。

实际生产Tauri/WebView/IPC/核心三路径通过：诊断复核需说明与勾选；过期摘要拒绝；产品按钮完整重启到Unknown；回读禁用、停止按钮隐藏、IPC拒绝；Unknown窗口关闭重开后仍未知；独立用户本地处置；正常停核后完整核心重启仍Retired。每路径两次完整核心重启加一次Unknown窗口重开，原文件不变、派发1/本地处置1/verified=false。原派发/停止传输为合成、原半写为同生产写入路径错误注入，未触碰真实Qoder/WorkBuddy或模型。最终reports/desktop-stop-review-native-yDlvLC/result.json，九图已查看。

首次原生脚本错误地期望Unknown任务允许正常停核，被现有保护拒绝；产品保护保留，脚本改为Unknown窗口重开及处置后完整核心重启，未以强制结束代替正常退出通过。首次失败证据reports/desktop-stop-review-native-0d1pJ5保留。最终及初次Job均回收后0进程（最终回收前2个，不能写成回收前0）。另reports/desktop-diagnostic-native-Nckkjk七类无批准链残片诊断回归，两次完整重启、原字节保持、复核按钮不可用通过，两图查看，Job回收前后均0。合计11图查看。

索引../../reports/p4-desktop-stop-review-review-20261010.json；选择性源码/文档归档，不是完整可重现构建归档，报告排除WebView缓存与凭证目录。041–048证据索引摘要逐项保持一致。实际凭证沿用025，无HEAD提交/推送/云CI；真实模型/外部消息/真实应用停止均0。

下一步仍有：停止复核凭据实际进程终止验证；原派发/完成回复/封存残片的独立恢复规则；产品调度联合真实UIA观察器中止与单次停止；真实Qoder编程/连续任务与WorkBuddy已有会话、真实OpenAI管理全链。全部首批API/CLI、P5异构worktree并行和P6安装卸载/升级回滚迁移/签名全范围保留。跨桥接版本未知恢复仍待处理；不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第009项（本日049）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-stop-review-review-20261010.json)

停止请求/动作意图/取消证明三类半写用户复核已接核心和界面，原批准/派发与前置证据约束；原文保留，只恢复本地管理，原尝试禁止继续。生产Tauri三路径复核→Unknown→用户本地处置→Retired通过，每路两次完整核心重启加一次Unknown窗口重开；原文件保持，无伪造取消或交付。原传输合成，半写错误注入；无真实模型/外部消息/应用停止。

417默认Rust/0失败/90忽略、5Node和fmt/严格clippy/契约/前端/核心/桌面构建通过；三新增专项包含在417内，15处复核发布为错误注入。原无批准链诊断原生回归通过，11图查看，三Job回收后0进程。首次脚本误期望Unknown允许停核，已修正测试，产品保护保留。三份runtime SHA：{runtime[0]['sha256']}。

下一步：停止复核凭据实际进程终止；其他派发/回复/封存残片恢复；生产调度联合真实UIA；实机Qoder/WorkBuddy、真实OpenAI全链。全部首批API/CLI、P5异构worktree并行及P6全范围保留。完整P0–P6未通过，目标active、本轮progress。卷003已有9项；凭证沿用025，无HEAD/推送/云CI，041–048索引有效。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'''

## 049 三类停止残片复核 {stamp}

原批准/派发事件和有效前置链约束的stop-request/stop-intent/stop-proof半写pending可由用户说明+确认另存复核。原文件保留，复核不改变工作流；重启核对后只恢复本地管理，原尝试结果未知且禁止继续回读/发送/停止。用户另外核对并结束本地跟踪，结果为Retired而不是取消成功。其他派发/回复/封存半写尚未开放。只读工作流检查不执行恢复或投影写入。

417默认Rust/90忽略、5Node及严格检查/构建通过，三阶段生产Tauri复核与处置各两次核心重启、Unknown窗口重开通过；无批准链诊断回归通过，11图查看，三Job回收后0。原传输合成，半写与15处复核发布为错误注入，未验证本批实际kill/断电或真实外部应用。见[049交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)及[证据](../reports/p4-desktop-stop-review-review-20261010.json)。完整P4/P6未通过。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8').replace('半写用户复核恢复（七类只读诊断已通过）；', '其余派发/回复/封存半写恢复、停止复核发布实际中断（三类停止残片复核和本地处置原生链已通过）；')
s += f'\n{stamp}第049项：三类停止半写复核保留原文，仅恢复本地管理；生产Tauri复核→Unknown→用户Retired三路径各两次核心重启和Unknown窗口重开通过，无批准链诊断回归通过，11图核查、三Job回收后0。417默认Rust/0失败/90忽略、5Node及严格检查/构建通过；15处复核发布为错误注入。真实模型/外部消息/应用停止0，其他半写恢复/实机全链/P5/P6仍未完成。见049交接及reports/p4-desktop-stop-review-review-20261010.json。\n'
matrix.write_text(s, encoding='utf-8')
selected = ['crates/core/src/' + name + '.rs' for name in ['scheduler', 'host', 'lib', 'diagnostic_service', 'desktop_execution', 'desktop_completion', 'desktop_continuity', 'desktop_cancellation', 'desktop_stop_review', 'desktop_stop_review_tests', 'desktop_background', 'desktop_stop_host', 'desktop_sessions', 'desktop_sessions_tests', 'desktop_disposition']]
selected += ['crates/domain/src/lib.rs', 'crates/domain/src/desktop_sessions.rs', 'apps/desktop/src/CoreDiagnostics.tsx', 'apps/desktop/src/DesktopStopRecordReview.tsx', 'apps/desktop/src/DesktopTasks.tsx', 'apps/desktop/src/contracts.ts', 'tests/desktop-stop-review-native.mjs', 'tests/desktop-diagnostic-native.mjs', 'tests/native-runtime.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/check.ps1', 'scripts/finalize-desktop-stop-review-20261010.py', 'docs/HANDOFF-LATEST.md', 'docs/DESKTOP-SESSION-BINDING-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for name in ['desktop-stop-review-native-0d1pJ5', 'desktop-stop-review-native-yDlvLC', 'desktop-diagnostic-native-Nckkjk', *jobs]:
    directory = reports / name
    paths.update(p for p in directory.rglob('*') if p.is_file() and not {'webview', 'credentials'}.intersection(p.relative_to(directory).parts))
paths.update(reports.glob('p4-desktop-stop-review-*.log'))
paths.update(reports / name for name in ['p4-desktop-stop-review-runtime.json', 'p4-desktop-stop-review-visual-review.json'])
record = {'createdAt': datetime.now(timezone.utc).isoformat(), 'checkpoint': 49, 'status': 'partial-stage-progress', 'scope': 'three partial desktop stop record reviews and local retirement; production Tauri/core; synthetic original transport', 'sourceArchive': 'selected only; not full reproducible build archive', 'rust': dict(zip(['passed', 'failed', 'ignored'], counts)), 'node': {'passed': 5, 'failed': 0}, 'fmt': 'passed', 'clippy': 'passed', 'frontendBuild': 'passed', 'coreBuild': 'passed', 'desktopBuild': 'passed', 'runtimeSha256': runtime[0]['sha256'], 'productionTestFixtures': False, 'nativeReport': visual['report'] + '/result.json', 'diagnosticRegression': visual['regression'] + '/result.json', 'nativeJobs': jobs, 'screenshotsReviewed': 11, 'publicationFaultInjectionCases': 15, 'actualReceiptWriterTerminations': 0, 'realModelCalls': 0, 'externalMessages': 0, 'externalAppStops': 0, 'p4': 'incomplete', 'p6': 'incomplete', 'previousIndexVerifiedCounts': old, 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 49, 'records': len(record['records']), 'rust': record['rust'], 'previousIndexes': old}))
