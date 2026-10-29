"""Freeze CLI/product evidence and retain the full original P0-P6 scope."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime
import json
import shutil
import re
import os

root=Path(__file__).resolve().parents[1]
if os.name=='nt' and not str(root).startswith('\\\\?\\'):
    root=Path('\\\\?\\'+str(root))
reports=root/'reports'
index=reports/'p3-workbench-cli-20261012.json'
archive=reports/'p3-workbench-cli-sources-20261012'
assert not index.exists() and not archive.exists(), 'Preserve frozen evidence'
def hash_file(p): return sha256(p.read_bytes()).hexdigest()
def load(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def log(stage):
    raw=(reports/f'p3-workbench-cli-{stage}-20261012.log').read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig')
previous=reports/'p5-shared-crash-20261012.json'
prior=load(previous)
for r in prior['records']: assert hash_file(root/r['snapshot'])==r['sha256'],r['snapshot']
changed={'crates/core/src/main.rs','crates/core/src/ipc.rs'}
for r in prior['records']:
    if r['source'].startswith('crates/') and r['source'] not in changed:
        assert hash_file(root/r['source'])==r['sha256'],r['source']
assert '4 passed; 0 failed' in log('unit4')
assert '305 passed; 0 failed; 155 ignored' in log('core')
assert re.search(r'(?:#|ℹ) pass 5\b',log('ipc')) and re.search(r'(?:#|ℹ) fail 0\b',log('ipc'))
for stage in ['clippy2','build3','desktop']:
    assert 'Finished' in log(stage) and 'error:' not in log(stage)
def result_from_log(stage):
    lines=[line for line in log(stage).splitlines() if line.startswith('{')]
    value=json.loads(lines[-1]); assert value['status']=='passed'
    normal=Path(value['report'])
    folder=root/'reports'/normal.name
    result=load(folder/'result.json');assert result['status']=='passed'
    return folder,result
native,native_result=result_from_log('native3')
startup,startup_result=result_from_log('startup')
assert native_result['results'][0]['credentialProviders']==6
assert native_result['results'][1]['configurationRoutes']==8
assert native_result['results'][1]['tasksAccepted']==2
assert native_result['results'][1]['reopenNewEvents']==0
assert startup_result['samePid'] and startup_result['cliPipesClosedWhileResidentAlive']
runtimes={n:hash_file(root/n) for n in prior['runtimeHashes']}
assert len(set(runtimes.values()))==1
stamp=datetime.now().astimezone().isoformat()
body=f'''093实现面向用户的统一工作台CLI，入口为已打包核心的 --workbench。项目登记/查询、验收器和配置文件、主指挥请求、完整方案审阅/摘要批准、启动/暂停/恢复、原尝试对账/取消、指挥分页详情/处置、独立验收详情/恢复均映射原用户IPC命令。没有第二调度器或批准路径；不自动选取摘要，只有service start可以启动核心，通信或输出失败不重发。

六个原凭证槽支持标准输入配置和删除；拒绝命令行Key参数，不回显输入或反序列化原文，暂存凭证缓冲清零。ProviderEndpoint仍是原官方端点闭合契约。保存配置不批准、不运行、不访问供应商；API/CLI订阅与桌面会话原界限不变。

本次实机命令行复用原离线API DAG服务：六个合成凭证槽、八条API配置保存路线、中文/空格项目路径、重复连接同一核心、无服务不隐式启动、未知参数/角色注入/旧摘要拒绝。完整两任务依赖修改结果为alpha beta，两次独立验收与主指挥Accept通过；关闭测试核心后以生产核心重开，原事件摘要保持，新增事件0。请求、核心、管道、文件和验收器是真实运行，模型和主管回复受控；六家真实认证/生成并未由此通过。

真实CLI暴露常驻启动句柄继承问题：核心返回成功但调用方输出管道不能结束，25秒超时。原首次脚本只看退出码，已补查子进程error，因此不能沿用首轮基本流程的通过断言。现在常驻启动使用CreateProcessW隐藏窗口且不继承句柄；沿原PID/SID握手、单实例文件锁、失败启动者清理，不加入用户进程Job、不改变ACL。新增中文/空格数据目录内两个并发CLI启动，原生管道捕获正常结束且核心继续存在，两者连接同PID，显式停止通过。

其他失败：首轮脚本把独立验收operationId误读为id，修正后详情及完整重开通过；严格clippy要求固定大小参数分组改用as_chunks，修正后四项CLI单元及实际并发启动通过。修改前后参数语义相同，原两任务专项沿用修正前已通过记录，不重复模型/桌面接入。失败日志均保存。

检查通过：CLI单元4项、核心305项/155项忽略、原stdio/常驻IPC共5项、workspace全targets严格clippy、核心和桌面custom-protocol debug构建。前端和其余Rust156项继承092/091，核心调度、观察、工作目录与桌面执行源码保持；本批实际CLI覆盖范围与取消/恢复底层历史证据在CLI说明中分开。三个runtime一致：{next(iter(runtimes.values()))}。冻结前复核092的{len(prior['records'])}份归档；旧冻结文件不修改。

完整P3/P4/P5/P6尚未验收，目标active。真实模型调用/用户桌面动作0，WorkBuddy064单次编程任务不重发。继续补真实OpenAI管理、全部首批真实接入、共享故障/负载、真实编程与含失败的效率；P6的安装/卸载、失败升级恢复、迁移备份、降级回滚、签名和支持矩阵仍全部保留。金额硬预算、按执行者监督策略及075/085未定观察也保持，不用CLI组件完成代替整阶段验收。
'''
latest=root/'docs/HANDOFF-LATEST.md';old=latest.read_text(encoding='utf-8-sig')
marker='当前：2026-10-12 卷002第001项（092）'
assert marker in old and '093最终检查已通过' not in old
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷002第002项（093），progress，093最终检查已通过。\n\n'+stamp+'\n\n'+body+'\n[CLI使用说明](WORKBENCH-CLI-V1.md)；[093验收索引](../reports/p3-workbench-cli-20261012.json)。\n\n下方092为冻结历史。\n\n'+old[old.index(marker):],encoding='utf8')
volumes=list((root/'handoffs/2026-10-12').glob('handoff_2026-10-12_002_*'));assert len(volumes)==1
volume=volumes[0];text=volume.read_text(encoding='utf8');assert '第002项' not in text
volume.write_text(text+'\n\n## 第002项 / 093 统一工作台CLI与常驻启动修复\n\n'+stamp+'\n\n'+body+'\n[093验收索引](../../reports/p3-workbench-cli-20261012.json)。\n',encoding='utf8')
matrix=root/'docs/STAGE-ACCEPTANCE-2026-10-12.md'
text=matrix.read_text(encoding='utf8').replace('本页092小节','本页093小节')
matrix.write_text(text+'\n\n## 093 统一工作台CLI（本批通过，完整目标active）\n\n'+stamp+'\n\n'+body,encoding='utf8')
gates=root/'docs/NEXT-STAGE-GATES.md';text=gates.read_text(encoding='utf8')
intro='''# 后续阶段的验收执行顺序

2026-10-12 / 093 更新：统一工作台CLI已接原用户IPC，配置/审阅批准/两任务独立验收/重开通过受控模型实机检查；修复常驻启动继承调用者输出句柄问题，并发启动保持同一核心。见[CLI说明](WORKBENCH-CLI-V1.md)、[最新交接](HANDOFF-LATEST.md)。完整P3/P4/P5/P6未通过。

- 下一步继续推进实际产品门槛：真实OpenAI主副指挥、全部首批API/CLI/桌面及真实编程与包含失败的效率；已验证接入先查预案，不重做基础验证。WorkBuddy064单次任务不重发。
- 共享故障/负载尚缺完整矩阵；092指定终止/恢复/双任务监督场景已通过。按执行者监督策略、金额硬预算及075/085未定观察仍保留。
- P6仍需可实际使用的安装/卸载、备份迁移、升级及失败恢复、回滚兼容、签名和支持矩阵。新schema2/binding2被旧版拒绝，不能静默降级。
- 原统一CLI缺口已实现，后续完善命令可用性与逐供应商真实验证；CLI组件通过不代替全部首批签收。
- 本日卷002当前第002项，旧冻结证据保留。

'''
gates.write_text(intro+text[text.index('以下为较早阶段的历史记录。'):],encoding='utf8')
paths={root/r['source'] for r in prior['records'] if r['source'].startswith('crates/')}
paths.update(root/n for n in ['crates/core/src/workbench_cli.rs','crates/core/src/resident_process.rs','scripts/check-workbench-cli-20261012.ps1','scripts/finalize-workbench-cli-20261012.py','tests/workbench-cli.test.mjs','tests/workbench-cli-startup.test.mjs','README.md','docs/WORKBENCH-CLI-V1.md','docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/STAGE-ACCEPTANCE-2026-10-12.md'])
paths.add(volume);paths.update(reports.glob('p3-workbench-cli-*-20261012.log'))
for folder in [native,startup,reports/'workbench-cli-8eYdyG',reports/'workbench-cli-ZjifeJ']:
    for p in folder.rglob('*'):
        if p.is_file() and 'credentials' not in p.relative_to(folder).parts and p.suffix in {'.json','.jsonl','.log'}:
            paths.add(p)
archive.mkdir();records=[]
for p in sorted(paths):
    rel=p.relative_to(root);dest=archive/rel;dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(p,dest);value=hash_file(p);assert hash_file(dest)==value
    records.append({'source':rel.as_posix(),'snapshot':dest.relative_to(root).as_posix(),'sha256':value})
report={'batch':'093','at':stamp,'status':'workbench-cli-and-resident-launch-verified','goalStatus':'active',
    'fullP3Accepted':False,'fullP4Accepted':False,'fullP5Accepted':False,'fullP6Accepted':False,
    'realModelCalls':0,'userDesktopActions':0,'newScheduler':False,'newApprovalAuthority':False,
    'corePassed':305,'coreIgnored':155,'cliUnitPassed':4,'nodeIpcPassed':5,'inheritedNonCoreRustPassed':156,
    'runtimeHashes':runtimes,'cliResult':native_result,'startupResult':startup_result,
    'remaining':['all real providers and OpenAI management','full shared fault/load and real coding/efficiency including failures','P6 install/uninstall/update/backup/migration/rollback/signing/support'],
    'previousIndex':previous.relative_to(root).as_posix(),'previousIndexSha256':hash_file(previous),
    'archiveScope':'selective source and evidence, not complete reproducible repository','records':records}
with index.open('x',encoding='utf8') as f: json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'index':str(index),'records':len(records),'runtimeSha256':next(iter(runtimes.values())),'goalStatus':'active'},ensure_ascii=False))
