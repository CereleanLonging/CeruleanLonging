"""One-time checkpoint 046; refuses to replace an existing evidence index."""
from datetime import datetime, timezone, timedelta
from pathlib import Path
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-cancellation-review-20261010.json'
archive = reports / 'p4-desktop-cancellation-sources-20261010'
assert not index.exists() and not archive.exists(), 'Do not rerun a frozen checkpoint'

def load(path):
    return json.loads((root / path).read_text(encoding='utf-8-sig'))

def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()

def log(path):
    raw = (root / path).read_bytes()
    return raw.decode('utf-16' if raw.startswith(b'\xff\xfe') else 'utf-8')

previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop']:
    original = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for row in original['records']:
        assert sha(row['path']) == row['sha256'], row['path']
    previous[label] = len(original['records'])

full = log('reports/p4-desktop-cancellation-check.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', full)]
counts = [sum(c[i] for c in counts) for i in range(3)]
assert counts == [411, 0, 87], counts
assert re.findall(r'(?:ℹ|#) pass (\d+)', full) == ['2', '3']
assert re.findall(r'(?:ℹ|#) fail (\d+)', full) == ['0', '0']
assert '40 passed; 0 failed; 11 ignored' in log('reports/p4-desktop-cancellation-focused-final.log')
assert '6 passed; 0 failed; 1 ignored' in log('reports/p4-desktop-cancellation-unit-final.log')
assert 'Finished' in log('reports/p4-desktop-cancellation-clippy-final.log')
assert 'Finished' in log('reports/p4-desktop-cancellation-desktop-final.log')
assert 'test-fixtures' not in log('reports/p4-desktop-cancellation-production-features.log')
runtime = load('reports/p4-desktop-cancellation-runtime.json')
for row in runtime:
    assert sha(row['path']) == row['sha256'], row['path']

native = ['desktop-cancellation-cancel-TIZJoy', 'desktop-cancellation-recover-ATsppO', 'desktop-cancellation-complete-apDpjF']
native_results = []
shots = []
for name in native:
    result = load(f'reports/{name}/result.json')
    assert result['status'] == 'passed' and result['dispatches'] == 1 and result['stopInvocations'] == 1
    assert result['realModelCalls'] == result['externalMessages'] == result['externalUserAppStops'] == 0
    native_results.append(result)
    shots += sorted(p.relative_to(root).as_posix() for p in (reports / name).glob('*.png'))
assert len(shots) == 14
jobs = {}
for path in sorted(reports.glob('desktop-cancellation-*-owned-20261010*')):
    cleanup = load(path.relative_to(root) / 'cleanup.json')
    assert cleanup['ExitCode'] == 0 and cleanup['RemainingProcesses'] == 0 and cleanup['CleanupConfirmed']
    jobs[path.relative_to(root).as_posix()] = cleanup
assert len(jobs) == 5

now = datetime.now(timezone(timedelta(hours=8)))
stamp = now.strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 046 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 046 桌面停止持久记录、后台取消与用户入口 {stamp}

上一轮与本轮均为progress。目标active，完整P0–P6未通过。本日46项，本卷6项。

新增desktop_cancellation与desktop_stop_host。用户确认框绑定原任务审阅摘要、项目、尝试、聊天和模型，要求原因与独立勾选。宿主先暂停该项目所有未终结运行，再将独立stop-request写入并同步；后台工作流所有权在外时，只修改这个独立请求文件并发出AtomicBool信号，中止本产品拥有的观察器等待。原线程结束并join、账本恢复后才转入停止工作；外部应用由原桥接的单次UIA Invoke处理，不作为本地观察器进程终止对象。等待中的请求和当前停止工作不可重复请求。

新请求成功发布才返回不可Clone/反序列化的ReservedRequest；重启从文件读不回动作资格。唯一后台写入者登记CancelIntent及Unknown，核对当前原TaskReview后只读准备StopObservation，再保存stop-intent，最后消费一次停止调用。没有可靠原观察、文件发布失败或动作失败，均保留原状态和记录，不重试点击。恢复路径只使用原stop-intent只读核对，缺少动作观察时只能回读完成结果；不会在重启后重新准备或补点停止。

stop-request、stop-intent、stop-proof各自带摘要且create_new/同步/非覆盖发布，正式/临时副本冲突或半写拒绝。验证原请求User身份、批准任务、工作流取消事件、观察身份/历史/时间/RuntimeId、双观察间隔和明确取消文字；证明发布后才用绑定原尝试的Cancelled receipt与Reconcile入账。缺少原请求/意图/证明、伪造成功终态、错误原消息、两观察间隔不足、已重算摘要的错误证明都拒绝。已确认取消与完成证明不能共存，取消后不允许补采完成或重新派发。相同完整pending仍可作为原证据读取，未实现损坏半写记录的用户复核修复。

完成竞争单独处理：如果原回复/封存已存在，不点击停止、不重新采样；否则停止无法确认时只读核对原完整完成回复。仅当前Unknown、有有效原CancelIntent及持久请求、没有取消证明的同一尝试允许既有封存流程继续；保持原文件封存、Submitted、独立Verify及chief Accept约束。没有放宽所有取消任务的提交限制。已完成先提交的路径可保留用户停止请求而不产生CancelIntent，展示真实完成结果。

界面可在后台任务执行时打开“停止原桌面任务”；停止后保持调度Paused，结果未确认时只提供回读/明确本地处置，取消确认显示“已确认取消，修改保留”。完成竞争显示待验收，显式恢复调度后才继续验收；Accepted文案同步为已收口。Rust停止传输入口从cfg(test)接入实际生产后台。真实停止能力依然限已适配Qoder版本；WorkBuddy未据此通过。

验证：411默认Rust/0失败/87忽略、5Node通过，生成契约、前端/核心构建通过。全量后补充缺少原请求的工作流一致性检查及枚举装箱，最终桌面专项40/0/11通过；再扩充3项缺失证据反例，最终取消专项6/0/1（1个原生服务fixture忽略）通过，未新增默认测试。fmt、最终严格clippy、最终前端及桌面debug构建通过，生产normal/build依赖无test-fixtures。三份runtime SHA：{runtime[0]['sha256']}。已知ts-rs属性解析提示保留。

三条最终原生路径均使用实际Tauri/WebView/IPC、后台所有权、持久账本及生产核心重开，桌面/管理回复为合成传输：

- reports/desktop-cancellation-cancel-TIZJoy：运行中用户确认停止，Paused、Cancelled/verified=false；1次派发、1次合成停止、2次合成管理请求，无Verify/Accept；生产核心重开保持。
- reports/desktop-cancellation-recover-ATsppO：停止回复丢失保持Unknown；后台结束后实际终止本测试核心并重开，原意图保持；只读回读确认取消，未重复停止，生产核心再重开保持。
- reports/desktop-cancellation-complete-apDpjF：停止时任务已完成，先Submitted/未验收且Paused；用户显式恢复调度后真实Node独立检查成功、合成chief接受。1次派发、1次合成停止、3次合成管理请求；生产核心重开Accepted/verified=true。原文件均保持。

最终三路14张截图已查看；5个本批专属Job（含图像同步复验）均退出0、回收后剩余0，部分退出时WebView子进程由Job清理。每路仅1次合成停止，不累计重复验证为更多通过场景。真实模型/外部消息/真实应用停止均0。本批没有把Tauri与真实WPF/UIA或真实Qoder接成同一次端到端测试：045已验证原生Invoke底层，本批验证生产调度接线；实际观察器中止信号和真实应用停止联合验证仍是后续项。

初次编译缺少TaskProof导入及旧轻量命令分支，已补齐；严格clippy要求大枚举装箱，修正后通过。原生三路状态断言首轮已过，但部分截图早于页面状态刷新，测试已增加明确标题等待；同时修复Accepted仍提示待验收的文案，复验通过。局部前端重建初次受沙箱子进程限制失败，使用本地构建权限后成功；失败日志保留，无自动审批拒绝。没有把这些首次失败标记为通过。

证据索引：../../reports/p4-desktop-cancellation-review-20261010.json；选择性源码/文档归档，非完整可重现构建归档。041–045旧索引93/104/135/153/339条摘要均一致。实际凭证沿用025；本批未读取其他应用凭证、未新增真实应用观察。无HEAD提交、推送或云CI。

下一步：停止请求/动作意图/取消证明发布各边界的实际宿主终止与半写恢复验收；生产调度联合真实UIA观察器中止和单次停止；随后真实Qoder编程/连续任务、WorkBuddy观察与派发、真实OpenAI管理链。全部首批API/CLI、P5异构worktree并行、P6干净机安装卸载/升级回滚迁移/签名保持全范围。跨桥接版本未知恢复仍待处理。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')

latest = f'''# 最新交接

[2026-10-10 卷003第006项（本日046）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面接入机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-cancellation-review-20261010.json)

已接入桌面用户停止入口、先暂停调度/持久用户请求、后台唯一所有权及观察器取消信号、一次停止动作资格、取消证明入账和重启后只读核对。完成竞争保留真实完成/文件封存/独立验收；取消与完成不混同。损坏半写证据仍拒绝，尚未实现其用户修复。

411默认Rust/0失败/87忽略、5Node、最终桌面40项、最终取消6项通过；fmt/最终严格clippy/契约/前端/核心/桌面构建通过。三条原生产品路径（取消、Unknown实际测试核心终止/重开后回读、完成竞争显式恢复后真实Node验收与合成chief收口）及生产核心重开通过，14图已检查，5个Job回收后0进程。模型与桌面传输仍合成，真实模型/外部消息/真实应用停止0。三份runtime SHA：{runtime[0]['sha256']}。

下一步：桌面停止发布边界实际中断/半写恢复、调度联合真实UIA观察器中止和单次停止，再推进实机Qoder编程/连续任务、WorkBuddy及真实OpenAI链。全部首批API/CLI、P5异构worktree并行和P6全范围保留；完整P0–P6未通过，目标active、本轮progress。卷003已有6项；实际凭证沿用025，无HEAD提交/推送/云CI；041–045旧索引有效，不要重跑已完成finalize脚本。
'''
(root / 'docs/HANDOFF-LATEST.md').write_text(latest, encoding='utf-8')

mechanism = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
mechanism.write_text(mechanism.read_text(encoding='utf-8').rstrip() + f'''

## 046 用户停止、后台所有权与持久恢复 {stamp}

生产CancelDesktopTask现已接入。用户核对原任务并填写原因/勾选后，宿主先暂停调度，独立保存stop-request；原观察器仅收到本地取消信号。原工作流线程退出并join后，唯一写入者登记CancelIntent和Unknown、准备原轮次观察、先发布stop-intent，再调用一次停止。动作资格是仅当次进程拥有的非Clone值，重启不重建，只读原观察/终态。

stop-request/stop-intent/stop-proof均带摘要及非覆盖发布；请求、事件、原观察和取消证明相互校验。只有同原任务双观察明确取消才Reconcile为Cancelled，保留已有文件，不能补采完成证明。若先完成，保留原回复/封存；停止不能确认时可只读核对真实完成。仅有效持久取消链的当前Unknown且无取消证明允许原封存提交，仍需独立验收和chief收口。调度保持Paused，须用户显式恢复或修订。

411默认Rust、5Node、最终40桌面专项与6取消专项通过；三条实际Tauri/IPC/持久记录/生产核心重开路径通过，14图已核查。外部模型及桌面回复仍合成；045的真实WPF/UIA底层与本批产品调度尚未联合成同一次原生测试，更不等于真实Qoder/WorkBuddy取消验收。发布边界实际终止、损坏半写修复、跨桥接版本恢复仍待验收。详见[046交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)和[证据](../reports/p4-desktop-cancellation-review-20261010.json)。
''', encoding='utf-8')

matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
text = matrix.read_text(encoding='utf-8')
text = text.replace('停止桥接受控WPF/UIA 17场景通过，取消须双观察证实 | 停止意图持久化、后台取消与用户入口、重启只读核对；', '停止桥接受控WPF/UIA 17场景通过；产品持久停止/用户入口、取消与完成竞争/重启只读核对原生链通过（合成桌面传输） | 停止发布边界实际中断/半写恢复；产品联合真实UIA观察器中止和停止；')
text += f'''
{stamp}第046项：用户停止、暂停调度、持久请求/单次动作资格、后台取消信号和重启只读核对已接入。411默认Rust/0失败/87忽略、5Node、最终40桌面及6取消专项、fmt/最终严格clippy/构建通过。实际Tauri取消、Unknown核心终止重开后只读回读、完成竞争显式恢复后真实Node验收/合成chief接受三条路径及生产核心重开通过，14图已查看，5Job回收后0进程。外部桌面/模型传输仍合成，真实调用/应用停止0；生产接线与真实UIA联合验收、发布边界中断及半写修复、实机Qoder/WorkBuddy/OpenAI和完整P5/P6仍待完成。见046交接及reports/p4-desktop-cancellation-review-20261010.json。
'''
matrix.write_text(text, encoding='utf-8')

prior_sources = load('reports/p4-desktop-stop-review-20261010.json')
selected = {r['path'].split('sources-20261010/', 1)[1] for r in prior_sources['records'] if 'sources-20261010/' in r['path']}
selected.update(['crates/core/src/desktop_cancellation.rs', 'crates/core/src/desktop_cancellation_tests.rs', 'crates/core/src/desktop_stop_host.rs', 'tests/desktop-cancellation-native.mjs', 'scripts/finalize-desktop-cancellation-20261010.py'])
for name in sorted(selected):
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)

paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in reports.glob('desktop-cancellation-*'):
    if directory.is_dir():
        paths.update(p for p in directory.rglob('*') if p.is_file() and 'webview' not in p.relative_to(directory).parts and 'credentials' not in p.relative_to(directory).parts)
paths.update(reports.glob('p4-desktop-cancellation-*.log'))
paths.add(reports / 'p4-desktop-cancellation-runtime.json')
record = {
    'createdAt': now.isoformat(), 'checkpoint': 46, 'status': 'partial-stage-progress',
    'scope': 'Production user cancellation, durable one-shot intent, exclusive background writer, read-only restart reconciliation and completed-race independent acceptance; desktop/OpenAI transports synthetic',
    'sourceArchive': 'selected files only; not a full reproducible build archive',
    'rust': dict(zip(['passed', 'failed', 'ignored'], counts)), 'node': {'passed': 5, 'failed': 0},
    'finalDesktopRust': {'passed': 40, 'failed': 0, 'ignored': 11},
    'finalCancellationRust': {'passed': 6, 'failed': 0, 'ignored': 1, 'note': 'additional missing-record assertions after full suite; ignored native server matches filter'},
    'fmt': 'passed', 'clippy': 'passed after enum boxing', 'frontendBuild': 'passed after permission-limited local build retry', 'coreBuild': 'passed', 'desktopBuild': 'passed',
    'runtimeSha256': runtime[0]['sha256'], 'productionTestFixtures': False,
    'nativeReports': [f'reports/{name}/result.json' for name in native], 'nativeJobs': jobs,
    'screenshotsReviewed': shots, 'remainingOwnedProcesses': 0,
    'realModelCalls': 0, 'externalMessages': 0, 'externalUserAppStops': 0,
    'nativeControlAndProductCombined': False, 'cancelPublicationCrashMatrix': 'not yet tested',
    'previousIndexVerifiedCounts': previous,
    'p4': 'incomplete', 'p6': 'incomplete',
    'remaining': ['desktop stop record publication crash/partial recovery matrix', 'production observer interruption plus real UIA stop integration', 'real Qoder programming/history/cancellation', 'WorkBuddy integration', 'real OpenAI management chain', 'all initial API/CLI integrations', 'P5 heterogeneous worktree concurrency', 'P6 install/uninstall/upgrade/rollback/migration/signing'],
    'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)],
}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for row in load(index)['records']:
    assert sha(row['path']) == row['sha256'], row['path']
print(json.dumps({'checkpoint': 46, 'records': len(record['records']), 'rust': record['rust'], 'nativeReports': record['nativeReports'], 'screenshots': len(shots), 'runtimeSha256': runtime[0]['sha256']}))
