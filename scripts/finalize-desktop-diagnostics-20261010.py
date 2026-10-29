"""Freeze checkpoint 048 once after native review; keep prior indexes immutable."""
from datetime import datetime, timezone, timedelta
from pathlib import Path
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-diagnostics-review-20261010.json'
archive = reports / 'p4-desktop-diagnostics-sources-20261010'
assert not index.exists() and not archive.exists(), 'Checkpoint already frozen'

def load(path):
    return json.loads((root / path).read_text(encoding='utf-8-sig'))

def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()

def log(path):
    raw = (root / path).read_bytes()
    return raw.decode('utf-16' if raw.startswith(b'\xff\xfe') else 'utf-8')

old = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication']:
    previous = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for item in previous['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    old[label] = len(previous['records'])
full = log('reports/p4-desktop-diagnostics-check.log')
rows = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', full)]
counts = [sum(row[i] for row in rows) for i in range(3)]
assert counts == [414, 0, 89], counts
assert re.findall(r'(?:ℹ|#) pass (\d+)', full) == ['2', '3']
assert re.findall(r'(?:ℹ|#) fail (\d+)', full) == ['0', '0']
for name in ['clippy', 'desktop']:
    assert 'Finished' in log(f'reports/p4-desktop-diagnostics-{name}.log')
assert 'test-fixtures' not in log('reports/p4-desktop-diagnostics-production-features.log')
runtime = load('reports/p4-desktop-diagnostics-runtime.json')
assert len(runtime) == 3 and len({item['sha256'] for item in runtime}) == 1
for item in runtime:
    assert sha(item['path']) == item['sha256'], item['path']
review = load('reports/p4-desktop-diagnostics-visual-review.json')
native_dir = root / review['report']
result = load(native_dir / 'result.json')
assert result['status'] == 'passed' and result['desktopFiles'] == 7 and result['fullCoreRestarts'] == 2
assert result['originalBytesPreserved'] and result['rawContentsHidden'] and result['mutationsRejected']
assert result['normalControlsHidden'] and result['stageLabelsRendered'] and result['gracefulCoreStop']
assert result['reviewableFiles'] == 0 and not result['partialRecoveryImplemented']
assert result['realModelCalls'] == 0 and review['screenshotsReviewed'] == ['diagnostic.png', 'reopened.png']
job_dir = root / review['job']
job = load(job_dir / 'cleanup.json')
assert job['ExitCode'] == 0 and job['RemainingProcesses'] == 0 and job['CleanupConfirmed'] and not job['TimedOut']
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 048 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 048 桌面残片只读诊断与两次原生核心重启 {stamp}

上一轮与本轮均为progress，目标active，完整P0–P6未通过。本日48项，本卷8项。

CoreDiagnostic新增兼容旧JSON的desktopFiles。桌面目录独立扫描上限512个尝试目录/8192个子项，只列64个文件，pending优先，每份最多读取131072字节；不递归进入嵌套目录，非普通文件不读取，超过扫描或读取范围明确显示。清单只包含原相对位置、写入状态、字节数和SHA-256，不返回正文或把未验证JSON里的任务身份当真。七类文件为派发意图、完成回复、封存意图、交付封存、停止请求、停止动作意图和取消证明；按目录识别待核对的尝试，项目/任务仍须原批准链确认。

本批只解决诊断可见性，没有新增半写复核或修复入口。完整JSON不等于任务完成/取消成功；普通计划、模型检查、任务停止和套用启动复核通道均拒绝。原生数据为隔离目录里手工构造的七份半写记录，不是重复执行047真实终止场景，亦不是原业务工作流的修复证明。

3项新增默认Rust测试覆盖七类记录两次打开/旧契约兼容/私密正文隐藏/原字节保持、独立清单限额和pending优先/大文件/嵌套目录不递归、缺失/无效根/空目录。嵌套目录测试不能等同真实符号链接或junction专项。全量414默认Rust/0失败/89忽略、5Node，以及fmt/严格clippy/契约生成/前端/核心/桌面debug构建通过；normal/build依赖不含test-fixtures。三份runtime SHA：{runtime[0]['sha256']}。已有ts-rs属性解析提示仍存在。

原生证据：{review['report']}/result.json，实际Tauri/WebView/IPC，生产核心打开诊断，七类标签和摘要可见；各导航仍锁定诊断，普通控制隐藏，四类不适用命令拒绝。通过产品按钮两次完整核心重启后七个文件及原字节保持、工作流/事件均0、正文不显示。两张截图已查看。专属Job退出0、回收后剩余0。真实模型/外部消息/真实应用停止均0。

索引：../../reports/p4-desktop-diagnostics-review-20261010.json。相关源码/文档仅选择性归档，不是完整可重现构建归档。041–047索引逐项摘要验证通过，记录数93/104/135/153/339/350/1334。未重新宣称旧原生流程已在新构建全部重跑。凭证状态沿用025，无HEAD提交/推送/云CI。

下一步：设计并实现基于原批准/原任务证据的桌面半写用户复核恢复，保留原文件且不得恢复发送/停止资格；完成产品调度联合真实UIA观察中止与停止验证。实机Qoder编程/连续任务、WorkBuddy、真实OpenAI管理链、全部首批API/CLI、P5异构worktree并行和P6安装卸载/升级回滚迁移/签名仍待验收。跨桥接版本未知恢复保留。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第008项（本日048）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面接入机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-diagnostics-review-20261010.json)

桌面派发/停止/封存七类记录已加入有界只读诊断：显示位置、写入状态和摘要，正文不进入界面，未验证身份不作为可信任务。实际Tauri窗口验证刷新及两次完整核心重启，原字节保持，普通执行和错误复核通道拒绝，两图已检查，专属Job回收后0进程。半写用户复核修复尚未实现。

414默认Rust/0失败/89忽略、5Node、fmt/严格clippy/契约/前端/核心/桌面构建通过；3新增专项包含在414内。三份runtime SHA：{runtime[0]['sha256']}。原生诊断使用隔离手工残片，真实模型/外部消息/应用停止0，不代表原业务修复或完整P4。

下一步：原批准链约束的半写用户复核恢复、生产调度联合真实UIA观察器中止和停止；再推进实机Qoder/WorkBuddy、真实OpenAI链。全部首批API/CLI、P5异构worktree并行及P6全范围保留。完整P0–P6未通过，目标active、本轮progress。卷003已有8项；实际凭证沿用025，无HEAD/推送/云CI；041–047索引有效。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'''

## 048 桌面只读诊断 {stamp}

七类派发/停止/封存记录进入独立有界清单：512目录/8192子项扫描、64项显示、每份131072字节，pending优先。只显示文件位置、写入状态和摘要；目录分组不等于项目/任务身份通过原批准链校验，完整JSON不等于完成或取消。无半写修复按钮。原生Tauri隔离手工七残片刷新及两次完整核心重启通过，原字节保留，正文隐藏、普通操作拒绝，两图查看、Job回收后0进程。

414默认Rust/0失败/89忽略、5Node与fmt/严格clippy/构建通过，真实模型/外部消息/应用停止0；非业务工作流修复验收。半写用户复核、产品联合真实UIA及实机全链仍待完成。见[048交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)及[证据索引](../reports/p4-desktop-diagnostics-review-20261010.json)。完整P4/P6未通过。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8').replace('桌面残片诊断清单、半写用户复核恢复；', '半写用户复核恢复（七类只读诊断已通过）；')
s += f'\n{stamp}第048项：七类桌面记录有界只读诊断，实际Tauri隔离手工残片两次完整核心重启通过，原字节保留/正文隐藏/普通操作拒绝，两图查看、Job剩余0。414默认Rust/0失败/89忽略、5Node及fmt/严格clippy/构建通过。仅诊断可见性，半写修复未实现；真实模型/外部消息/应用停止0，完整P4/P5/P6未通过。见048交接及reports/p4-desktop-diagnostics-review-20261010.json。\n'
matrix.write_text(s, encoding='utf-8')
selected = ['crates/domain/src/lib.rs', 'crates/core/src/diagnostic_service.rs', 'apps/desktop/src/CoreDiagnostics.tsx', 'apps/desktop/src/contracts.ts', 'tests/desktop-diagnostic-native.mjs', 'tests/native-runtime.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/test-codex-cancel-live.ps1', 'scripts/check.ps1', 'scripts/finalize-desktop-diagnostics-20261010.py', 'docs/HANDOFF-LATEST.md', 'docs/DESKTOP-SESSION-BINDING-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    destination = archive / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, destination)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in [native_dir, job_dir]:
    paths.update(p for p in directory.rglob('*') if p.is_file() and 'credentials' not in p.relative_to(directory).parts)
paths.update(reports.glob('p4-desktop-diagnostics-*.log'))
paths.update([reports / 'p4-desktop-diagnostics-runtime.json', reports / 'p4-desktop-diagnostics-visual-review.json'])
record = {'createdAt': datetime.now(timezone.utc).isoformat(), 'checkpoint': 48, 'status': 'partial-stage-progress', 'scope': 'bounded desktop record metadata diagnostics; native production core restarts on seven manually constructed partial files', 'sourceArchive': 'selected files only; not a full reproducible build archive', 'rust': dict(zip(['passed', 'failed', 'ignored'], counts)), 'node': {'passed': 5, 'failed': 0}, 'fmt': 'passed', 'clippy': 'passed', 'coreBuild': 'passed', 'frontendBuild': 'passed', 'desktopBuild': 'passed', 'runtimeSha256': runtime[0]['sha256'], 'productionTestFixtures': False, 'nativeReport': review['report'] + '/result.json', 'nativeJob': job, 'remainingOwnedProcesses': 0, 'screenshotsReviewed': 2, 'realModelCalls': 0, 'externalMessages': 0, 'externalUserAppStops': 0, 'partialRepairImplemented': False, 'p4': 'incomplete', 'p6': 'incomplete', 'previousIndexVerifiedCounts': old, 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 48, 'records': len(record['records']), 'rust': record['rust'], 'nativeReport': record['nativeReport'], 'runtimeSha256': runtime[0]['sha256']}))
