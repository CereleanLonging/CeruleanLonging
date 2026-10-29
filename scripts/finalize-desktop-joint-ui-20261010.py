"""Freeze checkpoint 053 once: Tauri -> IPC -> actual owned UIA cancellation."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-joint-ui-review-20261010.json'
archive = reports / 'p4-desktop-joint-ui-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))

def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()

def log(p):
    b = (root / p).read_bytes()
    return b.decode('utf-16' if b.startswith(b'\xff\xfe') else 'utf-8-sig')

previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review', 'stop-review-crash', 'joint']:
    old = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[label] = len(old['records'])
old = load('reports/p4-workbuddy-composer-review-20261010.json')
for item in old['records']:
    assert sha(item['path']) == item['sha256'], item['path']
previous['workbuddy-composer'] = len(old['records'])
for item in old['runtime']:
    assert sha(item['path']) == item['sha256'], item['path']
assert sha('crates/core/native/DesktopObserver.cs') == sha('reports/p4-workbuddy-composer-sources-20261010/crates/core/native/DesktopObserver.cs')
assert 'Finished' in log('reports/p4-desktop-joint-ui-compile.log')
assert 'Finished' in log('reports/p4-desktop-joint-ui-clippy.log')
jobs, cases, directories = {}, [], []
for scenario in ['success', 'unconfirmed']:
    for suffix in ['', '-02']:
        directory = reports / f'desktop-joint-ui-{scenario}-owned-20261010{suffix}'
        job = load(directory / 'cleanup.json')
        assert job['ExitCode'] == 0 and job['CleanupConfirmed'] and job['RemainingProcesses'] == 0
        jobs[directory.name] = job
        native = Path(json.loads(log(directory / 'stdout.log').strip())['report'])
        assert native.is_relative_to(reports)
        directories.extend([directory, native])
        result = load(native / 'result.json')
        assert result['status'] == 'passed' and result['realUia'] and not result['desktopFixtureUsed']
        assert result['dispatches'] == result['stopInvocations'] == 1
        assert result['productionCoreReopens'] == 2 and result['windowSurvived'] and result['originalBytesPreserved']
        assert not result['verified'] and not result['pageErrors']
        assert result['managementRequests'] == 2 and result['realModelCalls'] == result['externalUserMessages'] == result['externalUserAppStops'] == 0
        assert result['snapshotLatencyMs'] < 2000 and result['stopRequestLatencyMs'] < 3000 and result['stopCompletedMs'] < 25000
        assert result['originalState'] == ('cancelled' if scenario == 'success' else 'unknown')
        assert result['finalState'] == ('cancelled' if scenario == 'success' else 'retired')
        if suffix:
            cases.append({'report': native.relative_to(root).as_posix(), **result})
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 053 ' not in daily.read_text(encoding='utf-8')
summary = f'''Tauri界面批准→管理分配（合成OpenAI）→原任务审阅→真实UIA输入/单次发送→停止审阅→原观察取消→单次Stop→终态核对已联合通过。desktop_fixture=None，测试窗口由驱动独立拥有；配置来自生产发现/观察/二次关联，窗口PID/路径限定报告内WPF。实际界面、IPC、后台唯一写入、持久账本和UIA均参与，没有把受控窗口称为真实第三方应用。

确认取消路径：Cancelled/verified=false，快照{cases[0]['snapshotLatencyMs']}ms、停止确认界面返回{cases[0]['stopRequestLatencyMs']}ms、整个停止{cases[0]['stopCompletedMs']}ms；正常退出并两次生产核心重开保持。未确认路径：Unknown/verified=false，快照{cases[1]['snapshotLatencyMs']}ms、停止确认界面返回{cases[1]['stopRequestLatencyMs']}ms、整个停止{cases[1]['stopCompletedMs']}ms；后台结束后仅终止明确拥有的核心子进程两次，生产核心两次重开并实际UIA只读回查，仍Unknown。随后通过界面单独结束本地跟踪为Retired，不代表外部取消或完成。

每条实际发送1/停止1、管理请求2，调度Paused，无Submit/Verify/Accept、无独立验收成功。外部WPF窗口在核心重启中存活；原派发/停止文件摘要和预先存在的测试文件保持，旧发送/停止请求拒绝。原观察等待50秒，两条流程均提前完成；本批未单独记录原观察器PID终止证据。生产Tauri前台权限传递路径参与实际发送，未添加测试专用前台授权绕过。

最终证据{cases[0]['report']}和{cases[1]['report']}。初轮两条已通过，但一张截图抓在UI轮询更新前；补终态heading等待后两条重跑通过。最终17图已查看（13Tauri、4WPF）；WPF终态Group不显示文本，取消依据结构化UIA与Tauri状态，不能单靠按钮消失判断。四Job退出0/回收后0。

本批仅测试/文档改变。测试目标编译、fmt、核心all-targets严格clippy及四次显式原生运行通过；没有重复全量或产品构建，沿用052的417默认Rust/93忽略、5Node与完整构建；新增1个ignored服务入口不作为本轮全量统计。三runtime与052摘要一致，观察器源码未变。041–052旧索引摘要有效，源码选择性归档而非完整可重现构建。

真实用户消息/真实模型/真实用户应用停止0，无HEAD提交/推送/云CI，凭证沿用025。WorkBuddy已启动且052已完成实机观察，不再以未启动为阻碍；其稳定会话/任务发送/取消/重启连续性仍待适配。真实Qoder编程/连续任务和真实OpenAI主副指挥全链仍未验收。其他残片恢复、全部首批API/CLI、P5异构worktree并行、P6安装卸载/升级回滚迁移/签名全范围保留。'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + f'\n\n## 053 Tauri入口联合真实UIA发送与停止 {stamp}\n\n上一轮及本轮均progress，目标active，完整P0–P6未通过。本日53项、本卷13项。\n\n' + summary + '\n\n索引../../reports/p4-desktop-joint-ui-review-20261010.json。下一步优先推进真实Qoder批准编程任务/独立验收和WorkBuddy专用会话证据/动作适配。不要重跑已完成finalize脚本。\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第013项（本日053）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-joint-ui-review-20261010.json)

实际Tauri界面/IPC/生产后台联合真实WPF UIA发送与停止通过：确认取消为Cancelled，未确认保持Unknown；每条发送1/停止1、两次生产核心重开、外部测试窗口存活、原文件保持。Unknown只读回查仍未知后经独立用户入口Retired。管理合成、没有真实用户应用任务或模型；17图检查、四Job回收后0。

仅测试/文档修改；编译、fmt、核心严格clippy通过，全量/构建沿用052的417默认Rust/93忽略、5Node。三runtime/观察器与052一致，041–052旧索引有效。WorkBuddy已启动且可观察，发送及稳定会话身份尚待适配。

下一步：真实Qoder批准编程与独立验收、WorkBuddy会话/原任务证据及动作适配、真实OpenAI管理全链。其他残片恢复、全部首批API/CLI、P5/P6全范围保留，目标active。卷003已有13项；无HEAD/推送/云CI，凭证沿用025。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'\n\n## 053 产品入口联合真实UIA {stamp}\n\nTauri界面批准、管理分配、审阅发送、真实UIA单次输入发送、停止确认/后台观察取消/单次Stop联合两路径通过。明确取消为Cancelled，未确认保持Unknown；各两次生产核心重开，无重发，外部WPF存活，原记录保持。Unknown只读回查仍未知后独立用户Retired。管理响应合成、受控WPF非真实第三方应用，不能据此验收实机编程。17图检查、四Job剩余0；测试编译/严格clippy通过，产品与052一致。见053交接及[证据](../reports/p4-desktop-joint-ui-review-20261010.json)。完整P4/P6未通过。\n', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8').replace('Tauri入口联合真实UIA（宿主调度/后台与受控WPF真实输入/观察中止/单次停止已通过）；', '真实第三方应用端到端取消（Tauri/IPC/后台联合受控WPF UIA两路径及双核心重开已通过）；')
s += f'\n{stamp}第053项：Tauri/IPC/后台联合真实受控WPF输入/单次发送停止/终态通过，Cancelled与Unknown区分正确，各两次生产核心重开不重发；Unknown回查及独立Retired通过。17图检查、四Job剩余0；本批测试改动，编译/严格clippy通过，产品与052一致。真实第三方编程、真实OpenAI及完整P4/P5/P6仍待验收。见053交接及reports/p4-desktop-joint-ui-review-20261010.json。\n'
matrix.write_text(s, encoding='utf-8')
selected = ['crates/core/src/desktop_joint_native_tests.rs', 'crates/core/src/desktop_joint_ui_tests.rs', 'tests/desktop-joint-ui.mjs', 'tests/fixtures/DesktopJointWindow.cs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-desktop-joint-ui-20261010.py', 'docs/HANDOFF-LATEST.md', 'docs/DESKTOP-SESSION-BINDING-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file() and not {'credentials', 'webview'}.intersection(p.relative_to(directory).parts))
paths.update(reports.glob('p4-desktop-joint-ui-*.log'))
result = {'checkpoint': 53, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'cases': cases, 'jobs': jobs, 'screenshotsReviewed': 17, 'fullSuiteRerun': False, 'inheritedFullSuiteCheckpoint': 52, 'runtime': old['runtime'], 'realModelCalls': 0, 'externalUserMessages': 0, 'externalUserAppStops': 0, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected only; not a full reproducible build archive', 'previousIndexVerifiedCounts': previous, 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 53, 'records': len(result['records']), 'cases': len(cases), 'goal': 'active'}))
