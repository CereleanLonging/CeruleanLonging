"""Freeze 051 once: production host plus owned UIA; live observation is separate."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-desktop-joint-review-20261010.json'
archive = reports / 'p4-desktop-joint-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'
def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))
def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()
def log(p):
    b = (root / p).read_bytes()
    return b.decode('utf-16' if b.startswith(b'\xff\xfe') else 'utf-8')

old = {}
for label in ['background','management','continuity','recovery','stop','cancellation','stop-publication','diagnostics','stop-review','stop-review-crash']:
    previous = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for item in previous['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    old[label] = len(previous['records'])
production = []
prior = reports / 'p4-desktop-stop-review-sources-20261010'
for p in prior.rglob('*'):
    if p.is_file():
        rel = p.relative_to(prior)
        if rel.parts[0] in ['apps','crates'] and not rel.name.endswith('_tests.rs'):
            assert sha(p) == sha(rel), str(rel)
            production.append(rel.as_posix())
runtime = load('reports/p4-desktop-stop-review-runtime.json')
for item in runtime:
    assert sha(item['path']) == item['sha256']
assert 'test result: ok. 1 passed; 0 failed;' in log('reports/p4-desktop-joint-focused.log')
assert 'Finished' in log('reports/p4-desktop-joint-clippy-final.log')
native_dir = reports / 'desktop-joint-native-9PFiWR'
result = load(native_dir / 'result.json')
assert result['status'] == 'passed' and len(result['cases']) == 2
assert [c['finalState'] for c in result['cases']] == ['cancelled','unknown']
for c in result['cases']:
    assert c['realUia'] and not c['desktopFixtureUsed'] and c['windowSurvived'] and not c['verified']
    assert c['reopens'] == 2 and c['originalBytesPreserved'] and c['sendInvocations'] == c['stopInvocations'] == 1
    assert c['snapshotLatencyMs'] < 2000 and c['stopRequestLatencyMs'] < 2000 and c['stopCompletedMs'] < 25000
jobs = {}
for suffix, expected in [('',1),('-02',1),('-03',0)]:
    name='desktop-joint-owned-20261010'+suffix
    jobs[name]=load(f'reports/{name}/cleanup.json')
    assert jobs[name]['ExitCode']==expected and jobs[name]['CleanupConfirmed'] and jobs[name]['RemainingProcesses']==0
for suffix in ['', '-02']:
    name='desktop-joint-live-owned-20261010'+suffix
    jobs[name]=load(f'reports/{name}/cleanup.json')
    assert jobs[name]['ExitCode']==0 and jobs[name]['CleanupConfirmed'] and jobs[name]['RemainingProcesses']==0
live=load('reports/desktop-live-observation-pLb3fC/result.json')
assert live['readOnly'] and live['externalMessages']==0
assert any(w['window']['app']=='qoder' and w['observation'] is not None for w in live['windows'])
assert not any(w['window']['app']=='workbuddy' for w in live['windows'])
stamp=datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily=root/'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 051 ' not in daily.read_text(encoding='utf-8')
entry=f'''

## 051 生产宿主调度联合真实UIA发送/观察中止/停止 {stamp}

上一轮及本轮progress，目标active；完整P0–P6未通过。本日51项，本卷11项。

新增专属WPF测试窗口，具备真实UIA编辑器/发送/停止控件和可核对历史。配置来自生产发现→观察→二次关联，使用真实窗口/会话/模型/历史身份批准方案；管理响应仍合成。复用生产RuntimeHost命令、后台唯一写入、持久原派发、调度暂停、取消信号、原观察器进程回收、真实桥接单次Stop Invoke和终态核对。desktop_fixture=None，桌面响应不再由Rust合成返回。测试通过host.handle发送用户命令，未穿过Tauri前端/IPC，不夸大为完整产品原生窗口链。

两路径：success单次输入发送/停止后Cancelled；unconfirmed点击后控件消失但没有取消终态，保持Unknown。每条发送1/停止1、2个合成管理请求、无独立验收或chief Accept、verified=false、调度不再Active。停止请求分别11/21ms返回，快照1/0ms；原任务观察等待上限50秒，实际停止流程1618/11430ms完成，使用生产取消信号和进程runner，测试外部窗口仍存活。时间证明原流程提前结束，未单独记录原观察器PID终止事件，不把它写成独立PID取证。随后每条RuntimeHost重开两次，Unknown还经真实UIA只读核对，原文件相同、无重复发送或停止。

最终reports/desktop-joint-native-9PFiWR/result.json。四张WPF截图已检查发送后文本及停止控件变化；Group的终态标签由结构化UIA证明，截图不能单独证明取消。没有Tauri新截图。首次测试遗漏正式窗口发现步骤，被宿主拒绝；已补discover并断言实际枚举包含目标。第二次构建失败没有保存完整JSON编译输出，具体原因未定；后续增加构建失败日志保存，最终编译/联合运行/严格clippy通过，不把未取证原因当结论。三个专属Job均回收后0进程；最终Job回收前1个受控进程，回收后0。

另外只读实机观察：reports/desktop-live-observation-0fmivR显示Qoder 0.4.3.0最小化或不可见，未发现WorkBuddy。核对PID/启动时间/窗口所有者后ShowWindowAsync恢复Qoder，未输入或发送；reports/desktop-live-observation-pLb3fC已成功观察Qoder。两个只读Job退出0/回收后0。此结果只证明当前可观察，未新增聊天绑定、真实编程任务或停止真实应用；WorkBuddy仍待启动/窗口发现。窗口恢复记录reports/p4-desktop-joint-window-restore.json。

本批仅测试/文档变化，049选择性归档中19份生产源码与当前摘要一致，三份runtime仍{runtime[0]['sha256']}。管理持续运行专项1/0通过（覆盖正确/错误产物和2/3任务模式）、fmt/脚本语法/最终核心all-targets严格clippy通过；最终显式UIA两路径和实机只读测试通过。没有重跑全量/产品构建，沿用049的417默认Rust/90忽略、5Node；050新增2项ignored和本批1项ignored不计作本轮全量结果。已知ts-rs属性提示仍存在。

索引../../reports/p4-desktop-joint-review-20261010.json，选择性归档而非完整可重现构建；041–050旧证据摘要保持。真实模型/真实用户外部消息/真实用户应用停止0。无HEAD提交/推送/云CI，凭证沿用025。

下一步：将联合UIA取消链贯穿Tauri用户入口；基于现有真实Qoder窗口验证批准编程/连续任务与独立验收；补WorkBuddy已有会话适配及真实OpenAI管理链。原派发/回复/封存残片恢复、跨桥接版本未知恢复、全部首批API/CLI、P5异构worktree并行和P6安装卸载/升级回滚迁移/签名全范围保留。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip()+entry+'\n',encoding='utf-8')
(root/'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第011项（本日051）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-desktop-joint-review-20261010.json)

生产RuntimeHost调度/后台与实际WPF UIA输入发送、观察中止、单次停止联合两路径通过。确认取消→Cancelled，未确认→Unknown，每条发送1/停止1、两次重开、原文件保持、外部测试窗口存活。管理合成，调用host.handle而非Tauri/IPC，真实用户消息/模型/应用停止0；四图检查，五Job回收后0。

实机Qoder 0.4.3.0原最小化，身份核对后恢复窗口并成功只读观察；未发现WorkBuddy窗口。真实编程任务尚未发送。1项管理专项、fmt/脚本语法/最终核心严格clippy通过；仅测试改动，049生产源码19份与三runtime摘要匹配，全量沿用049的417默认Rust/90忽略、5Node及构建。未重跑全量。

下一步：Tauri入口联合UIA链、真实Qoder编程/连续任务、WorkBuddy和真实OpenAI全链。其他半写恢复、全部首批API/CLI、P5异构worktree并行、P6全范围保留。目标active，完整P0–P6未通过；卷003已有11项。凭证沿用025，无HEAD/推送/云CI；041–050索引有效。不要重跑已完成finalize脚本。
''',encoding='utf-8')
doc=root/'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip()+f'''

## 051 宿主调度联合真实UIA {stamp}

生产窗口发现/观察/绑定、批准/管理分配（合成指挥）、后台真实SendInput/Invoke、用户停止/暂停、观察取消、单次Stop与终态核对通过两条受控WPF路径。Confirmed为Cancelled，未确认保持Unknown，两次重开不补发，窗口存活、原文件保持、verified=false。用户命令走host.handle，尚未贯穿Tauri/IPC；WPF不是真实第三方应用，四图只证明控件/文本，终态依据结构化证据。管理专项、最终严格clippy及显式两路径通过，本批仅测试改动。

实际Qoder 0.4.3.0恢复窗口后只读观察成功；WorkBuddy窗口未发现，无真实消息。详见[051交接](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)及[证据](../reports/p4-desktop-joint-review-20261010.json)。完整P4/P6未通过。
''',encoding='utf-8')
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s=matrix.read_text(encoding='utf-8').replace('产品联合真实UIA观察器中止和停止；','Tauri入口联合真实UIA（宿主调度/后台与受控WPF真实输入/观察中止/单次停止已通过）；')
s+=f'\n{stamp}第051项：生产RuntimeHost调度/后台联合真实WPF UIA发送和停止，两路径每条发送1/停止1、Cancelled或Unknown正确、双重开原文保持。管理合成，尚未贯穿Tauri/IPC或真实第三方应用。四图检查、五Job剩余0，管理专项和最终严格clippy通过。真实Qoder恢复窗口后只读成功，未发现WorkBuddy；完整P4/P5/P6未完成。见051交接及reports/p4-desktop-joint-review-20261010.json。\n'
matrix.write_text(s,encoding='utf-8')
selected=['crates/core/src/desktop_run_tests.rs','crates/core/src/desktop_joint_native_tests.rs','crates/core/src/desktop_sessions.rs','crates/core/src/desktop_background.rs','crates/core/src/desktop_stop_host.rs','crates/core/src/desktop_cancellation.rs','crates/core/native/DesktopTrial.cs','crates/core/native/DesktopStop.cs','tests/fixtures/DesktopJointWindow.cs','tests/desktop-joint-native.mjs','tests/desktop-live-observation.mjs','scripts/test-omp-stop-review-native.ps1','scripts/finalize-desktop-joint-20261010.py','docs/HANDOFF-LATEST.md','docs/DESKTOP-SESSION-BINDING-V1.md','docs/STAGE-ACCEPTANCE-2026-10-10.md',daily.relative_to(root).as_posix()]
for name in selected:
    target=archive/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,target)
paths={p for p in archive.rglob('*') if p.is_file()}
for directory in [*reports.glob('desktop-joint-native-*'),reports/'desktop-live-observation-0fmivR',reports/'desktop-live-observation-pLb3fC',*(reports/name for name in jobs)]:
    paths.update(p for p in directory.rglob('*') if p.is_file() and not {'credentials','webview'}.intersection(p.relative_to(directory).parts))
paths.update(reports.glob('p4-desktop-joint-*.log'));paths.add(reports/'p4-desktop-joint-window-restore.json')
record={'createdAt':datetime.now(timezone.utc).isoformat(),'checkpoint':51,'status':'partial-stage-progress','scope':result['scope'],'nativeReport':native_dir.relative_to(root).as_posix()+'/result.json','cases':result['cases'],'jobs':jobs,'screenshotsReviewed':4,'liveObservation':'reports/desktop-live-observation-pLb3fC/result.json','fullSuiteRerun':False,'inheritedFullSuiteCheckpoint':49,'focusedRust':{'passed':1,'failed':0},'productionSourcesMatched':production,'runtimeSha256':runtime[0]['sha256'],'realModelCalls':0,'externalUserMessages':0,'externalUserAppStops':0,'tauriJointTested':False,'p4':'incomplete','p6':'incomplete','sourceArchive':'selected only; not a full reproducible build archive','previousIndexVerifiedCounts':old,'records':[{'path':p.relative_to(root).as_posix(),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path'])==item['sha256'],item['path']
print(json.dumps({'checkpoint':51,'records':len(record['records']),'productionSourcesMatched':len(production),'nativeCases':2,'liveQoderObserved':True}))
