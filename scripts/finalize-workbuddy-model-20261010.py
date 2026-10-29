"""Freeze 054 once after WorkBuddy 5.7.7 model observation and native UI checks."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-model-review-20261010.json'
archive = reports / 'p4-workbuddy-model-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'
def load(p):
    return json.loads((root / p).read_text(encoding='utf-8-sig'))
def sha(p):
    return hashlib.sha256((root / p).read_bytes()).hexdigest()
def log(p):
    b = (root / p).read_bytes()
    return b.decode('utf-16' if b.startswith(b'\xff\xfe') else 'utf-8-sig')
previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review', 'stop-review-crash', 'joint', 'joint-ui']:
    p = f'reports/p4-desktop-{label}-review-20261010.json'
    old = load(p)
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[label] = len(old['records'])
old = load('reports/p4-workbuddy-composer-review-20261010.json')
for item in old['records']:
    assert sha(item['path']) == item['sha256'], item['path']
previous['workbuddy-composer'] = len(old['records'])
names = ['desktop-capability-owned-20261010'+s for s in ['', '-02', '-03', '-04', '-05']] + ['workbuddy-navigation-owned-20261010', 'workbuddy-model-owned-20261010', 'workbuddy-model-live-owned-20261010', 'workbuddy-model-ui-owned-20261010']
jobs, native, directories = {}, {}, []
for name in names:
    directory = reports / name
    job = load(directory / 'cleanup.json')
    assert job['ExitCode'] == 0 and job['CleanupConfirmed'] and job['RemainingProcesses'] == 0, name
    jobs[name] = job
    native[name] = Path(json.loads(log(directory / 'stdout.log').strip())['report'])
    assert native[name].is_relative_to(reports)
    directories.extend([directory, native[name]])
cases = load(native['workbuddy-model-owned-20261010'] / 'result.json')
assert cases['status'] == 'passed' and len(cases['cases']) == 19
assert all(c['passed'] and c['inputChanges'] == 0 for c in cases['cases'])
assert next(c for c in cases['cases'] if c['mode'] == 'model-selected')['modelLabel'] == 'Hy3'
navigation = load(native['workbuddy-navigation-owned-20261010'] / 'result.json')['response']['result']
assert navigation['navigations'] == 1 and navigation['messagesSent'] == 0 and navigation['testLabels'] == 2
live = load(native['workbuddy-model-live-owned-20261010'] / 'result.json')
observed = next(w for w in live['windows'] if w['window']['app'] == 'workbuddy')
assert observed['window']['version'] == '5.7.7.0' and observed['error'] is None
assert observed['observation']['modelLabel'] == 'Hy3' and observed['observation']['sessionId'] is None
ui = load(native['workbuddy-model-ui-owned-20261010'] / 'result.json')
assert ui['status'] == 'passed' and 'workbuddy' in ui['liveAppsRead'] and not ui['pageErrors']
checks = log('reports/p4-workbuddy-model-check.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and sum(c[0] for c in counts) == 417 and sum(c[1] for c in counts) == 0 and sum(c[2] for c in counts) == 94
assert 'Finished' in log('reports/p4-workbuddy-model-clippy.log')
assert 'Finished' in log('reports/p4-workbuddy-model-desktop-build.log')
runtime = [{'path': p, 'sha256': sha(p)} for p in ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']]
assert len({v['sha256'] for v in runtime}) == 1
(reports / 'p4-workbuddy-model-runtime.json').write_text(json.dumps(runtime, indent=2)+'\n', encoding='utf-8')
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 054 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 054 WorkBuddy 5.7.7验证会话与模型显示名 {stamp}

上一轮及本轮progress，目标active；完整P0–P6未通过。本日54项、本卷14项。

实机状态已变化：Qoder已由旧PID20196重开为PID27064，最新观察存在“停止生成”，且不在已授权验证UUID 76edb3bc-078f-4ce4-bc9b-b6525fde9c51。未切换、未停止、未输入。WorkBuddy原进程已退出；确认无进程后启动安装路径D:/workbuddy/WorkBuddy.exe，发现实际版本已为5.7.7.0。默认隐藏启动后核对新PID12752/启动时间/路径及唯一Chrome主窗口并恢复显示；没有杀用户程序。最初文档尚未加载，稍后只读复查成功；未重启、未调整全局辅助功能。交互输入桌面为Default。

在空闲、无草稿时，两次生产观察摘要一致，并确认唯一可见按钮含精确“工作台接入验证”子文本，单次Invoke进入已有测试会话，随后同窗口/文档且标题出现于侧栏与会话头。导航1次、消息0次。证据{native['workbuddy-navigation-owned-20261010'].relative_to(root).as_posix()}。脚本只允许该固定测试聊天和5.7.7，不是产品任意导航接口。

选择控件明确提供ComboBox Name“Select model: Hy3”。生产DesktopObserver针对5.7.7提取唯一、可见、启用的该模型显示名，拒绝空白/控制字符/过长值，普通Text不计候选，重复有效候选不产生模型。该字符串是应用显示信息，非供应商模型认证；WorkBuddy observation.sessionId继续null。编程、固定文本发送及停止版本门槛未放开。

19类真实WPF/UIA验证全部通过，含原11种草稿/忙碌保护和8种模型候选情形；输入变化0。实际嵌入生产桥接读取WorkBuddy模型Hy3通过，证据{native['workbuddy-model-live-owned-20261010'].relative_to(root).as_posix()}。Tauri原生验证两个合成关联与生产核心重开，之后只读实际WorkBuddy并断言模型名通过IPC保持，四图已检查，页面错误0；真实测试聊天没有被关联到隔离产品项目，没有发送或调用模型。

完整scripts/check.ps1：417默认Rust/0失败/94忽略、5Node、fmt、契约/前端/核心构建通过；全workspace/all-targets/custom-protocol严格clippy及桌面debug构建通过。三runtime摘要一致：{runtime[0]['sha256']}。桥接摘要变化，旧批准仍须原版本复核，未绕过。九个测试Job退出0/回收后0；重新启动的用户WorkBuddy保留运行，不属于测试回收对象。

新增WORKBUDDY-ACTION-CONTRACT-V1.md写明真实5.7.7结构依据和下一步：独立消息轮次解析、至少两轮完整历史后缀、本地binding身份的回复/取消证据、单次动作保护，再接调度/连续任务。不能直接复用旧原型读取WorkBuddy Document.Value，不能只放宽版本开关（Rust回复验证当前仍要求Qoder会话UUID）。诊断仅保留固定控件标签、选择器文字与结构元数据，不保存聊天正文或WorkBuddy账号路由。

索引../../reports/p4-workbuddy-model-review-20261010.json，选择性源码归档而非完整可重现构建。041–053旧证据摘要有效；无真实用户消息/模型/应用停止，无HEAD提交/推送/云CI，凭证沿用025。Qoder当前任务忙碌不妨碍WorkBuddy工程推进，不标blocked。真实Qoder编程需等待指定聊天空闲并核对目录；真实OpenAI管理、其余残片恢复、全部首批API/CLI、P5异构worktree并行和P6完整范围继续保留。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip()+entry+'\n', encoding='utf-8')
(root/'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第014项（本日054）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [WorkBuddy动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md) · [本批证据](../reports/p4-workbuddy-model-review-20261010.json)

WorkBuddy已重新启动为5.7.7.0并进入既有“工作台接入验证”，导航1次/消息0次；新版模型控件“Select model: Hy3”已接生产观察，实机桥接与Tauri界面显示/IPC验证通过。19个真实UIA输入/模型保护场景通过，sessionId仍null，任务发送/取消尚待适配。Qoder新进程当前正在生成且不是原指定验证聊天，未操作。

417默认Rust/94忽略、5Node、fmt、全严格clippy、前端/核心/桌面debug构建通过；三runtime一致，四图查看，九测试Job回收后0。WorkBuddy用户应用保留运行；041–053旧索引有效。053的受控Tauri/UIA发送取消链继续有效，未提升为真实第三方编程验收。

下一步：按WorkBuddy动作约束实现轮次解析与本地binding回复/停止证据；真实Qoder在指定聊天空闲并核对目录后继续；真实OpenAI管理全链。其他残片恢复、全部首批API/CLI、P5/P6全范围保留，目标active，卷003已有14项。无HEAD/推送/云CI；不要重跑finalize脚本。
''', encoding='utf-8')
doc=root/'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip()+f'\n\n## 054 WorkBuddy 5.7.7模型观察 {stamp}\n\n实机进入既有验证聊天，模型ComboBox“Select model: Hy3”已接生产观察；限定版本、可见启用且唯一有效候选，19个真实UIA保护场景通过。生产桥接/Tauri模型显示名核对通过；无供应商会话ID，未开放任务发送/取消。完整417默认Rust/94忽略、5Node与检查/构建通过；四图查看、九Job回收后0。真实Qoder当前忙碌且为其他聊天，未操作。后续实现见[WorkBuddy动作约束](WORKBUDDY-ACTION-CONTRACT-V1.md)及054交接。完整P4/P6未通过。\n', encoding='utf-8')
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s=matrix.read_text(encoding='utf-8').replace('WorkBuddy稳定会话身份与发送（实机生产核心/Tauri只读观察已通过）；','WorkBuddy轮次/本地关联身份与发送（5.7.7验证聊天/Hy3模型显示名实机核心/Tauri观察已通过）；')
s+=f'\n{stamp}第054项：WorkBuddy已为5.7.7，进入既有验证聊天，模型显示名Hy3生产观察和Tauri展示通过，19个真实UIA保护场景通过。417默认Rust/94忽略、5Node、完整检查构建通过；四图、九Job回收后0。发送/原任务证据/稳定续接仍待适配；Qoder实际忙碌且非指定聊天未操作。完整P4/P5/P6未通过。见054交接及reports/p4-workbuddy-model-review-20261010.json。\n'
matrix.write_text(s,encoding='utf-8')
selected=['crates/core/native/DesktopObserver.cs','tests/desktop-workbuddy-composer.mjs','tests/fixtures/WorkBuddyComposerWindow.cs','tests/desktop-binding-native.mjs','tests/desktop-capability-diagnostic.mjs','tests/workbuddy-navigate-test.mjs','scripts/test-omp-stop-review-native.ps1','scripts/finalize-workbuddy-model-20261010.py','docs/HANDOFF-LATEST.md','docs/WORKBUDDY-ACTION-CONTRACT-V1.md','docs/DESKTOP-SESSION-BINDING-V1.md','docs/STAGE-ACCEPTANCE-2026-10-10.md',daily.relative_to(root).as_posix()]
for name in selected:
    target=archive/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(root/name,target)
paths={p for p in archive.rglob('*') if p.is_file()}
for directory in directories:
    paths.update(p for p in directory.rglob('*') if p.is_file() and not {'credentials','webview'}.intersection(p.relative_to(directory).parts))
paths.update(reports.glob('p4-workbuddy-model-*.log'))
for name in ['workbuddy-reopen-20261010.json','workbuddy-reopen-window-20261010.json','desktop-input-availability-20261010.json','p4-workbuddy-model-runtime.json']:
    paths.add(reports/name)
result={'checkpoint':54,'createdAt':datetime.now(timezone.utc).isoformat(),'status':'partial-stage-progress','workbuddyVersion':'5.7.7.0','observedModelLabel':'Hy3','modelIdentityAuthenticated':False,'navigationInvocations':1,'externalUserMessages':0,'realModelCalls':0,'externalUserAppStops':0,'nativeCases':19,'fullRust':{'passed':417,'failed':0,'ignored':94},'nodeTests':5,'screenshotsReviewed':4,'jobs':jobs,'runtime':runtime,'nativeUiReport':str(native['workbuddy-model-ui-owned-20261010'].relative_to(root))+'/result.json','p4':'incomplete','p6':'incomplete','previousIndexVerifiedCounts':previous,'sourceArchive':'selected only; not a full reproducible build archive','records':[{'path':p.relative_to(root).as_posix(),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path'])==item['sha256'],item['path']
print(json.dumps({'checkpoint':54,'records':len(result['records']),'goal':'active'}))
