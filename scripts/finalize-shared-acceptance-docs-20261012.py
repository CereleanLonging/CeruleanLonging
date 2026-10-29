"""Finish existing repository handoffs only after 087 acceptance preflight."""
from pathlib import Path
from datetime import datetime
import re

root = Path(__file__).resolve().parents[1]
freezer = root / 'scripts/finalize-shared-acceptance-20261012.py'
checked = {'__file__': str(freezer)}
exec(freezer.read_text(encoding='utf-8').split('archive.mkdir()')[0], checked)
runtime = next(iter(checked['runtimes'].values()))
seconds = {stage: re.findall(r'test result: ok\..*?finished in ([\d.]+)s', checked['read'](stage))[-1]
           for stage in ['runner', 'lifecycle4', 'verification', 'mixed']}
stamp = datetime.now().astimezone().isoformat()
body = '''087接通主指挥与API/独立验收共享账本的收口路径。原管理上下文、方案/批准、Submitted且verified的task/attempt、实际Verify证据与历史前缀必须一致。其他独立任务的生命周期事件允许交错，原目标或全局控制变化仍拒绝。宿主保存原ApplyIntent后，协调器内再次核对目标，再发布原Accept并保存原Applied回执；执行器Scope/Apply命令权限未扩大，没有新账本或历史格式迁移。原API历史前缀函数原样提取复用，已核对提取前后函数一致。

自动运行可以在B仍执行时验收并接受A，随后A的后续任务C沿原资源/额度/管理双ACK通道启动；模型槽仍全局2。共享验收、主指挥和API均通过同一所有者，主指挥网络请求不占写线程。已有ApplyIntent仅可读取原同操作事件进行对账；若没有事件，重复进入与重开均不补发。回执丢失后刷新原账本投影，管理状态仍保留Applying；显示已入账Accept不等于已收到管理确认。

新增检查使用实际Git/API线程/文件/独立验证子进程，主管和模型回复受控。自动运行4项5场景通过：两分支成功/失败、A收口后C在B执行时解锁、共享资源等待、第三独立任务补位。各任务只派发一次，正常4任务流程共9条管理请求，重开零新增事件。主指挥专项1项4场景通过：B交错完成后原A响应有效；Accept确认丢失后只读对账且投影及时更新；确认丢失后重开恢复原Applied；ApplyIntent存在而事件缺失时保留未决、零Accept/重发。过期响应、伪造历史快照、执行器伪造Accept以及协调器再次提交已改变目标均拒绝。

首轮夹具同时创建两条未处理管理请求，被原单请求保护正确拒绝；已改为原收口完成/重开后再提交过期响应夹具，生产约束未放宽，原失败日志保留。随后源码复核补齐回执丢失后的缓存刷新和缺失事件拒绝检查；自动运行初验154.61秒的证据继承，最终主指挥专项与全量回归覆盖新增变化。没有重复原桌面基础接入或WorkBuddy064原单次任务。
'''
body += (
    '\n最终检查：453项默认Rust/163项显式忽略（核心297/144），5项Node、前端与核心构建通过。'
    f'自动运行4项{seconds["runner"]}秒、主指挥1项4场景{seconds["lifecycle4"]}秒、'
    f'原共享验收1项5场景{seconds["verification"]}秒、混合宿主2项{seconds["mixed"]}秒通过。'
    '混合三种最终执行者各3任务Accepted、各派发一次、管理请求7次，中途/最终重开零新增事件；'
    '失败组合保留Unknown与文件、最终OMP零派发。严格workspace全targets clippy与桌面custom-protocol debug构建通过；'
    f'三个runtime SHA-256一致：{runtime}。\n\n'
    '最终补齐空闲/重开后已有Accept应用意图的同一禁止补发规则；补充核心297项及主指挥4场景复查，'
    '非核心默认检查继承本批首次全量，严格检查与桌面构建对应最终源码。'
    '原086的90份冻结归档及079的6份界面源文件摘要保持，工作目录/预算和原验证器未修改。'
    '本批真实模型调用/用户桌面动作0；同项目Codex/OMP并行生命周期、真实OpenAI管理、完整首批接入、'
    '实际异构编程与包含失败的效率、完整P6安装/升级/回滚/迁移/签名及支持矩阵仍未验收，目标active。'
    '075 Job退出观察和085投影共享冲突占用方未定结论保留。\n'
)
latest = root / 'docs/HANDOFF-LATEST.md'
text = latest.read_text(encoding='utf-8-sig')
marker = '当前：2026-10-12 卷001第015项（086）'
assert marker in text and '087最终检查已通过' not in text
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷001第016项（087），progress，087最终检查已通过。上一轮086为progress。\n\n'
    + stamp + '\n\n' + body + '\n[087验收索引](../reports/p5-shared-acceptance-20261012.json)。\n\n下方086为冻结历史。\n\n'
    + text[text.index(marker):], encoding='utf-8')
for name in ['docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md',
             'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md']:
    path = root / name
    text = path.read_text(encoding='utf-8-sig')
    assert '## 087 主指挥并行收口' not in text
    if name.startswith('docs/'):
        text = text.replace('更新至批次086', '更新至批次087').replace('本页086小节', '本页087小节')
    prefix = '../../' if name.startswith('handoffs/') else '../'
    heading = '## 第016项 / 087 主指挥并行收口' if name.startswith('handoffs/') else '## 087 主指挥并行收口'
    path.write_text(text + '\n\n' + heading + '（本批检查通过，完整目标active）\n\n' + stamp + '\n\n' + body
        + '\n[087验收索引](' + prefix + 'reports/p5-shared-acceptance-20261012.json)。\n', encoding='utf-8')
path = root / 'docs/NEXT-STAGE-GATES.md'
text = path.read_text(encoding='utf-8-sig')
start = text.index('以下为较早阶段的历史记录。')
lead = '''# 后续阶段的验收执行顺序

2026-10-12 / 087 更新（本批检查通过）：先读[最新交接](HANDOFF-LATEST.md)、[当日验收矩阵](STAGE-ACCEPTANCE-2026-10-12.md)与[P5接入记录](P5-WORKTREE-IMPLEMENTATION.md)。完整P4/P5/P6仍未验收，旧记录保留当时边界。

- 084/085的API公开并行与自动运行、086的共享独立验收、087的共享主指挥Accept及依赖解锁均已通过相应受控检查。复用原批准、管理双ACK、原验证器、应用事务和单写账本，不重做基础接入。
- 下一步迁移Codex/OMP共享启动、准备、派发、取消和恢复。当前async_codex/async_omp的Done仍归还DurableWorkflow，活动表仍按项目；不能只删独占检查。managed_assignment::assign已接受WorkflowAccess，可复用双ACK；codex_execution/omp_execution及监督/取消仍依赖DurableWorkflow和原路径/锚点方法，须逐项抽出受限能力，保持原native turn或controller-request身份。
- 协调器现有DispatchIntent关口调用API专属dispatch_reservation，不能把CLI直接接入该预算函数。应保留API原子轮数预留，同时明确CLI一次派发与原请求计数/未决保留规则。执行客户端不得获得Verify/Accept/批准能力；主指挥087专用通道保持宿主持有。各线程按(project,attempt)取消、最后一个使用者退出才归还账本；Startup前缀、双ACK、原始回执和旧版本重开格式均需保留。
- 对照P3完整首批名单，按已有真实证据和实现差异补真实OpenAI管理及API/选定厂商CLI能力；模型ID以真实账号返回为准。旧Qoder/WorkBuddy原型成功保留，补产品版本/绑定/编程/可靠性缺口；WorkBuddy064原单次授权已使用，不重发旧sum任务。
- 真实串行与并行编程在共同规范、基准目录和独立验收下比较，完整记录失败/返工/管理成本/实际调用用量和人工时间。受控场景不能代替真实效率。
- P6保持干净目标安装/卸载、核心与桥接升级/失败恢复、备份/数据迁移、旧版本回滚拒绝不兼容新数据、签名与支持矩阵和全部首批承诺签收。debug构建和选择性源码归档不代证。

'''
path.write_text(lead + text[start:], encoding='utf-8')
print('087 handoff and full next-stage gates finalized; prior frozen records preserved.')
