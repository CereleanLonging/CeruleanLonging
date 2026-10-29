"""Freeze checkpoint 047 exactly once; prior indexes remain immutable."""
from datetime import datetime, timezone, timedelta
from pathlib import Path
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-stop-publication-review-20261010.json'
archive = reports / 'p4-desktop-stop-publication-sources-20261010'
assert not index.exists() and not archive.exists(), 'Checkpoint already frozen'

def load(path):
    return json.loads((root / path).read_text(encoding='utf-8-sig'))

def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()

def log(path):
    raw = (root / path).read_bytes()
    return raw.decode('utf-16' if raw.startswith(b'\xff\xfe') else 'utf-8')

old = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation']:
    j = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for r in j['records']:
        assert sha(r['path']) == r['sha256'], r['path']
    old[label] = len(j['records'])
full = log('reports/p4-desktop-stop-publication-check.log')
rows = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', full)]
counts = [sum(r[i] for r in rows) for i in range(3)]
assert counts == [411, 0, 89], counts
assert re.findall(r'(?:ℹ|#) pass (\d+)', full) == ['2', '3']
assert re.findall(r'(?:ℹ|#) fail (\d+)', full) == ['0', '0']
assert 'Finished' in log('reports/p4-desktop-stop-publication-clippy-final.log')
assert 'Finished' in log('reports/p4-desktop-stop-publication-desktop.log')
assert 'test-fixtures' not in log('reports/p4-desktop-stop-publication-production-features.log')
runtime = load('reports/p4-desktop-stop-publication-runtime.json')
for r in runtime:
    assert sha(r['path']) == r['sha256'], r['path']
assert len({r['sha256'] for r in runtime}) == 1
final = 'reports/desktop-stop-publication-VZIeY1/result.json'
result = load(final)
assert result['status'] == 'passed' and result['actualTerminations'] == len(result['cases']) == 20
assert all(c['reopens'] == 2 and c['actualTermination'] and c['originalBytesPreserved'] and c['recoveryStopInvocations'] == 0 for c in result['cases'])
assert sum(c['diagnostic'] for c in result['cases']) == 6
assert sum(c['finalState'] == 'unknown' for c in result['cases']) == 8
assert sum(c['finalState'] == 'cancelled' for c in result['cases']) == 6
assert sum(c['stopInvocations'] for c in result['cases']) == 8
jobs = {}
for name in ['desktop-stop-publication-owned-20261010', 'desktop-stop-publication-owned-20261010-02']:
    j = load(f'reports/{name}/cleanup.json')
    assert j['ExitCode'] == 0 and j['RemainingProcesses'] == 0 and j['CleanupConfirmed']
    jobs[name] = j
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 047 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 047 桌面停止发布20处实际中断与双重开验收 {stamp}

上一轮与本轮均为progress，目标active，完整P0–P6未通过。本日47项，本卷7项。

desktop_cancellation停止记录写入增加可观察发布边界：独立create_new预留、前半段写入、完整写入并sync_all、hard_link非覆盖发布、删除pending。stop-request、stop-intent、stop-proof共15处；另覆盖CancelIntent已入账、Unknown已入账、单次停止返回、取消证明验证后但未保存、Cancelled已对账共5处。生产调用传入空观察回调，测试使用同一写入路径在指定边界等待父进程终止，不依靠返回模拟错误代表实际中断。

最终reports/desktop-stop-publication-VZIeY1/result.json包含20个独立案例。每例在reports下使用新目录，由专属子进程创建真实RuntimeHost、原批准/分配/派发及用户停止请求；后台停止传输为合成。子进程同步ready标记，父进程核对实际PID与指定边界后终止它并wait确认退出。之后各重开两次。不是断电/系统崩溃测试，也不是在真实Qoder/WorkBuddy点击停止。

结果分布：6个空白或半写请求/意图/证明拒绝RuntimeHost正常打开，ServiceHost进入锁定诊断，普通取消命令拒绝；两次重开前后原工作流事件集合与原记录字节完全一致，未实现用户修复。8个已完整保存但尚未执行停止的场景保持Unknown，没有补准备、补点停止或补发消息。6个停止已经返回或原取消证明已经完整保存的场景，通过原证明或只读回读进入Cancelled/verified=false，第二次重开不增加重复事件。全组实际合成stop调用8次（包括2个取消证明半写而保持诊断的场景），恢复stop调用0；原派发各1次，正常恢复各CancelIntent仅1次，无Submit/Verify/Accept。

补严验证：后台会捕获panic，因此仅让禁止传输分支panic不足以证明未调用。最终增加跨回调原子计数，确保恢复路径从未尝试prepare/send/stop；同时比较诊断路径完整工作流事件集合，防止“打开失败但已修改其他事件”被漏检。最终原生中断批次实际执行上述断言通过。原记录清单及字节在两次重开中保持，完整pending允许作为原证据读取但不会恢复停止动作资格；半写记录保持原样且不被解释为取消成功。

验证：411默认Rust/0失败/89忽略、5Node、fmt、最终严格clippy、生成契约/前端/核心/桌面debug构建通过；本批新增2个显式ignored测试（写入进程辅助、20处中断父测试）。新增发布写入先经取消专项6/0/1通过；全量后仅加强ignored中断测试的计数和事件断言，最终显式20例通过，不重复累计首次20例。normal/build依赖无test-fixtures；三份runtime SHA：{runtime[0]['sha256']}。已知ts-rs属性解析提示仍存在。

两个专属Job均退出0、回收后剩余0；最终Job退出前还有1个受控进程由Job清理，不能写成退出前已经0。本批无新增产品界面和截图验证，046三条原生产品取消/恢复/完成竞争证据仍保留；本批没有重新宣称046全部产品界面在新二进制上跑过。无真实模型调用、外部消息或真实用户应用停止。

证据索引：../../reports/p4-desktop-stop-publication-review-20261010.json。相关源码/文档为选择性归档，不是完整可重现构建归档；041–046旧索引93/104/135/153/339/350条摘要保持一致。实际凭证状态沿用025，未读取其他应用凭证；无HEAD提交、推送或云CI。当前实际应用状态沿用045只读观察，不以旧观察推断现在窗口是否存在。

下一步优先让诊断界面识别桌面派发/停止/封存残片，展示有界原文件证据并设计明确用户复核恢复，不删除原记录或恢复一次停止资格；同时推进产品调度联合真实UIA观察器中止和单次停止。随后实机Qoder编程/连续任务、WorkBuddy观察/派发、真实OpenAI管理全链。全部首批API/CLI、P5异构worktree并行、P6干净机安装卸载/升级回滚迁移/签名保持全范围。跨桥接版本未知恢复仍待处理；不要重跑既有finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第007项（本日047）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面接入机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-stop-publication-review-20261010.json)

停止请求/动作意图/取消证明15个写入发布边界，加工作流/传输5处，共20个实际专属写入进程终止通过；每例重开两次。6例空白/半写保持诊断且原工作流事件不变，8例Unknown不补停止，6例依据原证明或只读核对Cancelled。恢复从未尝试prepare/send/stop，原文件保持。半写用户复核修复尚未实现。

411默认Rust/0失败/89忽略、5Node、最终显式20例、fmt/最终严格clippy/契约/前端/核心/桌面构建通过；两个Job回收后0进程。真实模型/外部消息/真实应用停止0，本批没有新界面截图。三份runtime SHA：{runtime[0]['sha256']}。

下一步：桌面派发/停止/封存诊断清单与用户复核恢复、生产调度联合真实UIA观察器中止和停止；再推进实机Qoder/WorkBuddy、真实OpenAI链。全部首批API/CLI、P5异构worktree并行及P6全范围保留。完整P0–P6未通过，目标active、本轮progress。卷003已有7项；实际凭证沿用025，无HEAD/推送/云CI；041–046索引有效。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'''

## 047 停止发布边界实际中断 {stamp}

三份停止记录各覆盖预留、半写、同步、非覆盖发布和pending收尾，另覆盖CancelIntent、Unknown、停止返回、证明验证和Cancelled入账。20个实际专属写入进程终止，每例两次RuntimeHost/ServiceHost重开通过：6半写诊断、8未知不补发、6原证明/只读回读取消。后台捕获异常不能掩盖违规调用，恢复传输额外用原子计数验证prepare/send/stop均0；诊断场景工作流事件集合和全部原记录字节保持。没有断电验证或真实外部应用停止。

411默认Rust/89忽略、5Node、fmt/最终严格clippy/构建与显式20例通过，最终Job回收后0进程。仅验证损坏记录被保留并拒绝继续，用户半写复核/修复和可见诊断清单仍待实现；生产调度联合真实UIA尚待验证。见[047交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)及[本批证据](../reports/p4-desktop-stop-publication-review-20261010.json)。完整P4/P6仍未通过。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8')
s = s.replace('产品持久停止/用户入口、取消与完成竞争/重启只读核对原生链通过（合成桌面传输） | 停止发布边界实际中断/半写恢复；', '产品持久停止/用户入口、取消与完成竞争/重启只读核对原生链通过（合成桌面传输）；停止发布20处实际进程终止及双重开通过 | 桌面残片诊断清单、半写用户复核恢复；')
s += f'''\n{stamp}第047项：停止三份记录15处写入发布及工作流/传输5处实际写入进程终止通过，每例重开2次。6半写诊断、8未知不补发、6原证明/只读回读取消；原文件及诊断工作流事件保持，恢复prepare/send/stop尝试0。411默认Rust/0失败/89忽略、5Node、最终显式20例、fmt/严格clippy/构建通过，两Job回收后0进程。无真实应用停止/模型请求；半写用户修复、产品联合真实UIA及全范围P4/P5/P6仍未完成。见047交接及reports/p4-desktop-stop-publication-review-20261010.json。\n'''
matrix.write_text(s, encoding='utf-8')
selected = {r['path'].split('sources-20261010/', 1)[1] for r in load('reports/p4-desktop-cancellation-review-20261010.json')['records'] if 'sources-20261010/' in r['path']}
selected.update(['crates/core/src/desktop_stop_publication_tests.rs', 'tests/desktop-stop-publication.mjs', 'scripts/finalize-desktop-stop-publication-20261010.py'])
for name in sorted(selected):
    destination = archive / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, destination)
paths = {p for p in archive.rglob('*') if p.is_file()}
for name in ['desktop-stop-publication-ZgSeor', 'desktop-stop-publication-VZIeY1', *jobs]:
    directory = reports / name
    paths.update(p for p in directory.rglob('*') if p.is_file() and 'credentials' not in p.relative_to(directory).parts)
paths.update(reports.glob('p4-desktop-stop-publication-*.log'))
paths.add(reports / 'p4-desktop-stop-publication-runtime.json')
record = {
    'createdAt': datetime.now(timezone.utc).isoformat(), 'checkpoint': 47, 'status': 'partial-stage-progress',
    'scope': '20 actual owned desktop cancellation writer terminations; 15 publication and 5 workflow/transport boundaries; two reopens each; synthetic transport',
    'sourceArchive': 'selected files only; not a full reproducible build archive',
    'rust': dict(zip(['passed', 'failed', 'ignored'], counts)), 'node': {'passed': 5, 'failed': 0},
    'initialFocusedRust': {'passed': 6, 'failed': 0, 'ignored': 1},
    'explicitCrashTest': {'passed': 1, 'actualTerminations': 20, 'reopensPerCase': 2, 'diagnostic': 6, 'unknown': 8, 'cancelled': 6, 'syntheticStopInvocations': 8, 'recoveryForbiddenTransportCalls': 0},
    'fmt': 'passed', 'clippy': 'passed', 'coreBuild': 'passed', 'frontendBuild': 'passed', 'desktopBuild': 'passed',
    'runtimeSha256': runtime[0]['sha256'], 'productionTestFixtures': False,
    'nativeReport': final, 'nativeJobs': jobs, 'remainingOwnedProcesses': 0,
    'screenshotsReviewed': 0, 'realModelCalls': 0, 'externalMessages': 0, 'externalUserAppStops': 0,
    'partialRepairImplemented': False, 'powerLossTested': False, 'p4': 'incomplete', 'p6': 'incomplete',
    'previousIndexVerifiedCounts': old,
    'remaining': ['desktop record diagnostics and partial review/recovery', 'production observer interruption with real UIA stop', 'real Qoder programming/history/cancellation', 'WorkBuddy integration', 'real OpenAI management chain', 'all initial API/CLI integrations', 'P5 heterogeneous worktree concurrency', 'P6 install/uninstall/upgrade/rollback/migration/signing'],
    'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)],
}
index.write_text(json.dumps(record, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for r in load(index)['records']:
    assert sha(r['path']) == r['sha256'], r['path']
print(json.dumps({'checkpoint': 47, 'records': len(record['records']), 'rust': record['rust'], 'actualTerminations': 20, 'runtimeSha256': runtime[0]['sha256']}))
