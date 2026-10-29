"""Freeze checkpoint 052 once, after live and native UI evidence is complete."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re
import shutil

root = Path(__file__).resolve().parent.parent
reports = root / 'reports'
index = reports / 'p4-workbuddy-composer-review-20261010.json'
archive = reports / 'p4-workbuddy-composer-sources-20261010'
assert not index.exists() and not archive.exists(), 'Already frozen'

def load(path):
    return json.loads((root / path).read_text(encoding='utf-8-sig'))

def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()

def log(path):
    data = (root / path).read_bytes()
    return data.decode('utf-16' if data.startswith(b'\xff\xfe') else 'utf-8-sig')

previous = {}
for label in ['background', 'management', 'continuity', 'recovery', 'stop', 'cancellation', 'stop-publication', 'diagnostics', 'stop-review', 'stop-review-crash', 'joint']:
    old = load(f'reports/p4-desktop-{label}-review-20261010.json')
    for item in old['records']:
        assert sha(item['path']) == item['sha256'], item['path']
    previous[label] = len(old['records'])

jobs = {}
evidence_dirs = []
for name, code in [
    ('desktop-workbuddy-live-owned-20261010', 0),
    ('desktop-workbuddy-live-owned-20261010-02', 0),
    ('desktop-workbuddy-diagnostic-owned-20261010', 0),
    ('desktop-workbuddy-diagnostic-owned-20261010-02', 0),
    ('desktop-workbuddy-diagnostic-owned-20261010-03', 0),
    ('desktop-workbuddy-composer-owned-20261010', 1),
    ('desktop-workbuddy-composer-owned-20261010-02', 0),
    ('desktop-workbuddy-ui-owned-20261010', 0),
]:
    directory = reports / name
    result = load(directory / 'cleanup.json')
    assert result['ExitCode'] == code and result['CleanupConfirmed'] and result['RemainingProcesses'] == 0, name
    jobs[name] = result
    evidence_dirs.append(directory)
    if code == 0:
        record = json.loads(log(directory / 'stdout.log').strip())
        evidence_dirs.append(Path(record['report']))

live = load('reports/desktop-live-observation-UEJsy7/result.json')
assert live['readOnly'] and live['externalMessages'] == 0
assert {w['window']['app'] for w in live['windows'] if w['observation'] and w['error'] is None} == {'qoder', 'workbuddy'}
diagnostic = load('reports/desktop-composer-diagnostic-uc3xBx/result.json')['observations'][0]['lines']
assert any(v.get('editors') == 2 and v['normalizedKnownPlaceholder'] for v in diagnostic)
native = load('reports/desktop-workbuddy-composer-hlHnIz/result.json')
assert native['status'] == 'passed' and len(native['cases']) == 11
assert all(c['passed'] and c['inputChanges'] == 0 for c in native['cases'])
ui_dir = Path(json.loads(log('reports/desktop-workbuddy-ui-owned-20261010/stdout.log').strip())['report'])
ui = load(ui_dir / 'result.json')
assert ui['status'] == 'passed' and set(ui['liveAppsRead']) == {'qoder', 'workbuddy'}
assert ui['externalMessages'] == ui['modelCalls'] == 0 and not ui['pageErrors']
checks = log('reports/p4-workbuddy-composer-check.log')
counts = [tuple(map(int, m)) for m in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', checks)]
assert counts and sum(c[0] for c in counts) == 417 and sum(c[1] for c in counts) == 0 and sum(c[2] for c in counts) == 93
assert 'Finished' in log('reports/p4-workbuddy-composer-clippy.log')
assert 'Finished' in log('reports/p4-workbuddy-composer-desktop-build.log')
runtime_names = ['target/debug/cerulean-core.exe', 'target/debug/cerulean-runtime.exe', 'apps/desktop/src-tauri/binaries/cerulean-runtime-x86_64-pc-windows-msvc.exe']
runtime = [{'path': p, 'sha256': sha(p)} for p in runtime_names]
assert len({r['sha256'] for r in runtime}) == 1
(reports / 'p4-workbuddy-composer-runtime.json').write_text(json.dumps(runtime, indent=2) + '\n', encoding='utf-8')
stamp = datetime.now(timezone(timedelta(hours=8))).strftime('%H:%M +0800')
daily = root / 'handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md'
assert '## 052 ' not in daily.read_text(encoding='utf-8')
entry = f'''

## 052 WorkBuddy 实机输入框识别修复与产品读取 {stamp}

用户已启动WorkBuddy，任务转入实际窗口核查。本日52项、本卷12项，目标active；完整P0–P6未通过。

修复前reports/desktop-live-observation-UUXcPP发现WorkBuddy 5.7.6.0（PID28332）却拒绝观察。生产桥接同源诊断确认：480节点，两个可见Edit，其中一个22×22、值为空；真正输入框740×64，值27字符，是既知占位文本外包U+FEFF及换行。原规则将辅助Edit累计、又未规范化占位包装，因此触发draft_or_composer_unavailable。中途窗口最小化，核对PID/启动时间/可执行路径/HWND所有者后恢复；没有点击聊天或输入。直接PowerShell UIA枚举只见2节点/Document不可用，不能作为聊天结构结论，已改用独立.NET Framework观察器取证。

生产DesktopObserver.cs只排除有可读且完全空值、正尺寸且宽高均不超过32的WorkBuddy辅助Edit；真实非空辅助输入不排除。聊天输入仅剥离首尾FEFF/CR/LF，仍要求单一输入框且空或精确既知占位文本；不trim空格、不清理真实草稿、不放宽忙碌检查。Qoder输入匹配规则保持。该尺寸规则是当前实机版本的兼容处理，不宣称其他版本均兼容。

受控WPF真实UIA 11场景通过（reports/desktop-workbuddy-composer-hlHnIz）：占位/空/纯包装空可读；真实草稿/带包装草稿/单空格/辅助草稿/较大第二输入/无聊天输入/重复聊天输入拒绝；忙碌拒绝。所有输入变化计数0。首次夹具编译缺System.Xaml引用，补齐后通过；失败Job及日志保留，不计成功。最终实际生产观察reports/desktop-live-observation-UEJsy7对WorkBuddy和Qoder均成功。

实际Tauri/IPC原生验证{ui_dir.relative_to(root).as_posix()}：两个合成关联审阅、历史保存与生产核心重开通过；随后在生产核心中逐一发现并只读实际WorkBuddy/Qoder，窗口身份与预读一致，两张真实观察卡截图及三张合成关联/恢复图已查看，页面错误0。真实聊天没有绑定到测试项目、没有发送消息或调用模型。WorkBuddy sessionId仍null，不能据此宣称既有聊天的稳定身份/重启恢复已解决；产品发送/编程/停止尚未开放WorkBuddy。

完整scripts/check.ps1通过：417默认Rust/0失败/93忽略、5Node、fmt、契约生成、前端和核心构建；workspace/all-targets/custom-protocol严格clippy及桌面debug构建通过。三runtime摘要一致：{runtime[0]['sha256']}。原生观察器摘要已改变；旧批准记录按原桥接版本校验，未绕过该检查或宣称跨版本迁移通过。八个专属Job最终剩余进程均0；首次夹具构建失败后回收正常。

索引../../reports/p4-workbuddy-composer-review-20261010.json，源文件选择性归档而非完整可重现构建。041–051旧证据摘要一致。真实用户外部消息/模型/应用停止0；未读取其他应用凭证、无HEAD提交/推送/云CI。

下一步恢复Tauri入口联合受控真实UIA发送/停止验证；随后推进WorkBuddy稳定会话/原任务证据及单次动作适配、真实Qoder编程/连续任务、真实OpenAI管理全链。WorkBuddy已启动且可观察，不能继续写成“未启动”。其余损坏恢复、全部首批API/CLI、P5异构worktree并行及P6全范围保留。不要重跑已完成finalize脚本。
'''
daily.write_text(daily.read_text(encoding='utf-8').rstrip() + entry + '\n', encoding='utf-8')
(root / 'docs/HANDOFF-LATEST.md').write_text(f'''# 最新交接

[2026-10-10 卷003第012项（本日052）{stamp}](../handoffs/2026-10-10/handoff_2026-10-10_003_102620+0800.md)

[阶段矩阵](STAGE-ACCEPTANCE-2026-10-10.md) · [桌面机制](DESKTOP-SESSION-BINDING-V1.md) · [本批证据](../reports/p4-workbuddy-composer-review-20261010.json)

WorkBuddy 5.7.6.0已启动并完成生产核心及Tauri界面只读观察，Qoder同时通过。修复辅助小输入框和占位文本FEFF/换行造成的误判；11个真实WPF/UIA保护场景通过，草稿/歧义/忙碌仍拒绝。用户聊天未发送、未绑定测试项目。WorkBuddy没有可靠sessionId，发送/编程/停止及重启连续性仍待适配。

417默认Rust/93忽略、5Node、fmt、严格clippy、前端/核心/桌面debug构建通过，三runtime摘要一致；八Job回收后0，五图已检查。041–051旧索引有效。上一批宿主联合真实UIA仍未贯穿Tauri发送/取消入口。

下一步：Tauri入口联合真实UIA、WorkBuddy稳定会话与原任务证据、真实Qoder编程及OpenAI管理全链。其他残片恢复、全部首批API/CLI、P5/P6全范围保留，目标active。卷003已有12项，无HEAD/推送/云CI；凭证沿用025。不要重跑已完成finalize脚本。
''', encoding='utf-8')
doc = root / 'docs/DESKTOP-SESSION-BINDING-V1.md'
doc.write_text(doc.read_text(encoding='utf-8').rstrip() + f'''

## 052 WorkBuddy 实机观察修复 {stamp}

WorkBuddy 5.7.6.0的空辅助Edit（22×22）和占位文本首尾FEFF/换行使旧观察规则误拒绝。现仅排除正尺寸不超过32且值可读并完全空的辅助Edit，规范化已知包装后仍检查唯一聊天输入、真实草稿及忙碌。11个受控真实UIA保护场景通过；生产核心和实际Tauri界面均成功只读WorkBuddy/Qoder，用户聊天零输入/零发送。无可靠WorkBuddy会话ID；未开启其发送/编程/停止或声称重启连续性通过。

417默认Rust/93忽略、5Node及严格检查/构建通过，八Job回收后0，五图检查。观察器版本摘要更新，旧批准仍要求原桥接版本。详见052交接及[证据](../reports/p4-workbuddy-composer-review-20261010.json)。完整P4/P6未通过。
''', encoding='utf-8')
matrix = root / 'docs/STAGE-ACCEPTANCE-2026-10-10.md'
s = matrix.read_text(encoding='utf-8').replace('WorkBuddy实机观察和发送；', 'WorkBuddy稳定会话身份与发送（实机生产核心/Tauri只读观察已通过）；')
s += f'\n{stamp}第052项：修复WorkBuddy辅助Edit/占位包装误判，11个真实UIA保护场景及实机WorkBuddy/Qoder双观察通过，生产Tauri读取确认。417默认Rust/93忽略、5Node、严格检查与构建通过；外部消息0。WorkBuddy发送/稳定身份/连续性、Tauri联合真实UIA发送停止和完整P4/P6仍未通过。见052交接及reports/p4-workbuddy-composer-review-20261010.json。\n'
matrix.write_text(s, encoding='utf-8')
selected = ['crates/core/native/DesktopObserver.cs', 'tests/desktop-composer-diagnostic.mjs', 'tests/desktop-workbuddy-composer.mjs', 'tests/fixtures/WorkBuddyComposerWindow.cs', 'tests/desktop-binding-native.mjs', 'scripts/test-omp-stop-review-native.ps1', 'scripts/finalize-workbuddy-composer-20261010.py', 'docs/HANDOFF-LATEST.md', 'docs/DESKTOP-SESSION-BINDING-V1.md', 'docs/STAGE-ACCEPTANCE-2026-10-10.md', daily.relative_to(root).as_posix()]
for name in selected:
    target = archive / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / name, target)
paths = {p for p in archive.rglob('*') if p.is_file()}
for directory in evidence_dirs:
    assert directory.is_relative_to(reports)
    paths.update(p for p in directory.rglob('*') if p.is_file() and not {'credentials', 'webview'}.intersection(p.relative_to(directory).parts))
paths.update(reports.glob('p4-workbuddy-composer-*.log'))
paths.update(reports.glob('workbuddy-*-20261010.json'))
paths.add(reports / 'p4-workbuddy-composer-runtime.json')
result = {'checkpoint': 52, 'createdAt': datetime.now(timezone.utc).isoformat(), 'status': 'partial-stage-progress', 'realApplicationsObserved': ['workbuddy', 'qoder'], 'nativeUiReport': ui_dir.relative_to(root).as_posix() + '/result.json', 'ownedUiaCases': 11, 'fullRust': {'passed': 417, 'failed': 0, 'ignored': 93}, 'nodeTests': 5, 'screenshotsReviewed': 5, 'runtime': runtime, 'jobs': jobs, 'realModelCalls': 0, 'externalUserMessages': 0, 'externalUserAppStops': 0, 'workbuddyTaskSendEnabled': False, 'p4': 'incomplete', 'p6': 'incomplete', 'sourceArchive': 'selected only; not a full reproducible build archive', 'previousIndexVerifiedCounts': previous, 'records': [{'path': p.relative_to(root).as_posix(), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in sorted(paths)]}
index.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for item in load(index)['records']:
    assert sha(item['path']) == item['sha256'], item['path']
print(json.dumps({'checkpoint': 52, 'records': len(result['records']), 'liveApps': result['realApplicationsObserved'], 'goal': 'active'}))
