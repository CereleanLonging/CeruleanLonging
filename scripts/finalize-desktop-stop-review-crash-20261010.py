"""Freeze checkpoint 050 once after the owned reviewer termination test."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-stop-review-crash-review-20261010.json'
archive = reports / 'p4-desktop-stop-review-crash-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'
def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))
def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()
def log(p):
    b = (root / p).read_bytes()
    return b.decode('utf-16' if b.startswith(b'\xff\xfe') else 'utf-8')

previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review']:
    old = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[label] = len(old['records'])
# This checkpoint changes only test code and documentation. Verify production
# sources against the selected 049 source archive before inheriting its builds.
prior_archive = reports / 'p4-desktop-stop-review-sources-20261010'
production = []
for p in prior_archive.rglob('*'):
    if p.is_file():
        rel = p.relative_to(prior_archive)
        if rel.parts[0] in ['apps', 'crates'] and not rel.name.endswith('_tests.rs'):
            assert sha(p) == sha(rel), str(rel)
            production.append(rel.as_posix())
runtime = load('reports/p4-desktop-stop-review-runtime.json')
for item in runtime:
    assert sha(item['path']) == item['sha256']
assert 'test result: ok. 3 passed; 0 failed; 3 ignored;' in log('reports/p4-desktop-stop-review-crash-focused.log')
assert 'Finished' in log('reports/p4-desktop-stop-review-crash-clippy.log')
jobdir = reports / 'desktop-stop-review-crash-owned-20261010'
job = load(jobdir / 'cleanup.json')
assert job['ExitCode'] == 0 and job['CleanupConfirmed'] and job['RemainingProcesses'] == 0 and not job['TimedOut']
line = json.loads(log(jobdir / 'stdout.log').strip())
native = Path(line['report'])
assert native.resolve().is_relative_to(reports.resolve())
result = load(native / 'result.json')
assert result['status'] == 'passed' and result['actualTerminations'] == len(result['cases']) == 15
assert len({(c['stage'], c['point']) for c in result['cases']}) == 15
assert sum(c['initialDiagnostic'] for c in result['cases']) == 9
for c in result['cases']:
    assert c['actualTermination'] and c['preReviewReopens'] == c['postReviewReopens'] == 2 and c['retirementReopens'] == 1
    assert c['originalBytesPreserved'] and c['interruptedReceiptBytesPreserved'] and c['reviewDoesNotChangeWorkflow']
    assert c['transportCalls'] == 0 and c['finalState'] == 'retired' and not c['verified']
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 050 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 050 停止复核凭据15处真实进程终止 {stamp}

上一轮与本轮均为progress，目标active；完整P0–P6未通过。本日50项，本卷10项。

补齐049复核发布的真实进程终止证据：stop-request/stop-intent/stop-proof原半写分别对应复核凭据created/partial/synced/published/finished五处，共15例。专属写入子进程持有与服务一致的基础锁，调用生产acknowledge写入；同步ready标记包含PID/阶段/边界/原身份，父进程核对后仅终止该子进程并wait确认退出。不是仅抛出错误模拟此次复核中断，也不是断电测试。原停止残片仍来自生产写入路径错误注入，原派发/停止传输合成；不能称为真实外部应用链中断。

15例每例先两次重开：9个未发布凭据保持诊断，不取得恢复资格；6个已发布凭据恢复Unknown/verified=false，但停止与回读均拒绝。每次对比原工作流事件集合，保持不变；原桌面记录与中断复核凭据全部原字节保留。显式用户复核重试后再重开两次，正式凭据恰好一份、旧半写凭据保留、工作流不增加事件；桌面传输计数0。随后独立用户RetireUnknownDesktop，并再次重开验证Retired/verified=false，原派发1次、本地处置1次，没有Submit/Verify/Accept/Cancelled。总计每例2次复核前重开+2次复核后重开+1次处置后重开。

本轮仅测试/文档改动，049选择性归档中的生产源码与当前逐项摘要相同，三份runtime仍为{runtime[0]['sha256']}。验证为fmt、核心all-targets严格clippy、新脚本语法、3项复核专项通过/0失败/3忽略，加显式15例通过。没有重新运行全量，沿用049的417默认Rust/0失败/90忽略、5Node和产品构建；新增2项ignored测试令当前清单应为92忽略，但不把推算当作本轮全量结果。本轮未新增UI或重新执行049产品窗口验收。已知ts-rs属性提示仍存在。

证据：{native.relative_to(root).as_posix()}/result.json；专属Job退出0/回收后0进程，回收前数量以cleanup.json为准。真实模型/外部消息/真实应用停止0，无新截图。索引../../reports/p4-desktop-stop-review-crash-review-20261010.json；选择性归档，不是完整可重现构建归档；041–049原索引逐项摘要保持。无HEAD提交/推送/云CI，凭证沿用025。

下一阶段转入生产调度联合真实UIA观察器中止与停止，随后实机Qoder编程/连续任务、WorkBuddy已有会话和真实OpenAI管理全链。原派发/完成回复/封存残片的独立恢复规则、跨桥接版本未知恢复仍保留；全部首批API/CLI、P5异构worktree并行、P6安装卸载/升级回滚迁移/签名全范围不缩减。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第010项（本日050）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-stop-review-crash-review-20261010.json)

三类停止残片的复核凭据15处真实专属写入进程终止通过。每例复核前/后各重开两次，再用户本地处置后重开；9未发布保持诊断，6已发布恢复Unknown，重试保持原文件及中断凭据，工作流不因复核增事件，传输调用0，最终Retired/verified=false。

本轮仅测试/文档变化：3专项通过、fmt/核心严格clippy/脚本语法及显式15例通过，Job回收后0进程。049归档生产源码与当前匹配，三份runtime仍{runtime[0]['sha256']}。全量沿用049的417默认Rust/90忽略、5Node与构建，没有本轮全量重跑或新UI截图。原停止残片错误注入、原传输合成，真实模型/外部消息/应用停止0。

下一步：生产调度联合真实UIA观察器中止和停止；实机Qoder/WorkBuddy、真实OpenAI全链。其他派发/回复/封存残片恢复、全部首批API/CLI、P5异构worktree并行及P6全范围保留。完整P0–P6未通过，目标active、本轮progress。卷003已有10项；实际凭证沿用025，无HEAD/推送/云CI；041–049索引有效。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'''

## 050 停止复核凭据实际中断 {stamp}

三类停止残片×五复核发布边界共15个实际专属写入进程终止，每例复核前后各两次重开、独立用户处置后再重开。9未发布保持诊断，6已发布恢复Unknown；显式重试只发布一份正式凭据，原字节及中断凭据保留，复核不改工作流事件，传输0，最终Retired/verified=false。原停止残片错误注入、原传输合成，非断电或真实外部应用链。

测试/文档改动；3专项、fmt/核心严格clippy及显式15例通过，Job回收后0。049生产源码/三runtime摘要保持，沿用其全量和构建。产品联合真实UIA、其他残片恢复、实机全链与P5/P6仍待完成。见[050交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)及[证据](../reports/p4-desktop-stop-review-crash-review-20261010.json)。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8').replace('其余派发/回复/封存半写恢复、停止复核发布实际中断（三类停止残片复核和本地处置原生链已通过）；', '其余派发/回复/封存半写恢复（三类停止残片复核/本地处置原生链与15处复核发布实际进程终止已通过）；')
s += f'\n{stamp}第050项：停止复核三阶段×五发布边界共15次实际写入进程终止，每例复核前/后双重开及处置后重开，原证据/中断凭据保持，复核不改工作流，传输0。3专项和严格检查/显式15例通过；仅测试改动，049生产源码/runtime摘要一致，全量沿用049。无真实外部应用/模型、新截图；产品联合真实UIA与完整P4/P5/P6仍待验收。见050交接及reports/p4-desktop-stop-review-crash-review-20261010.json。\n'
matrix.write_text(s, encoding='utf-8')
selected = ['crates/core/src/desktop_stop_review_tests.rs', 'crates/core/src/desktop_stop_review_crash_tests.rs', 'crates/core/src/desktop_stop_review.rs', 'tests/desktop-stop-review-crash.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-desktop-stop-review-crash-20261010.py', 'docs/HANDOFF-LATEST.md', 'docs/DESKTOP-SESSION-BINDING-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in [native, jobdir]:
    paths.update(p for p in directory.rglob('*') if p.is_file() and 'credentials' not in p.relative_to(directory).parts)
paths.update(reports.glob('p4-desktop-stop-review-crash-*.log'))
record = {'createdAt':datetime.now(timezone.utc).isoformat(), 'checkpoint':50, 'status':'partial-stage-progress', 'scope':'15 actual owned stop review receipt writer terminations', 'actualTerminations':15, 'initialDiagnosticCases':9, 'initialUnknownCases':6, 'reopensPerCase':{'beforeReview':2,'afterReview':2,'afterRetirement':1}, 'focusedRust':{'passed':3,'failed':0,'ignored':3}, 'inheritedFullSuiteCheckpoint':49, 'fullSuiteRerun':False, 'productionSourceFilesVerifiedAgainst049':production, 'runtimeSha256':runtime[0]['sha256'], 'nativeJob':job, 'nativeReport':(native/'result.json').relative_to(root).as_posix(), 'screenshotsReviewed':0, 'realModelCalls':0, 'externalMessages':0, 'externalAppStops':0, 'p4':'incomplete','p6':'incomplete', 'sourceArchive':'selected files only; not a full reproducible build archive', 'previousIndexVerifiedCounts':previous,'records':[{'path':p.relative_to(root).as_posix(),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2)+'\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint':50,'records':len(record['records']),'actualTerminations':15,'productionSourcesMatched':len(production)}))
