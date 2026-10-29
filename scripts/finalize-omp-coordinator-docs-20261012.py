"""Update repository handoffs after batch 088 acceptance preflight."""
from pathlib import Path
from datetime import datetime
import re

root = Path(__file__).resolve().parents[1]
freezer = root / 'scripts/finalize-omp-coordinator-20261012.py'
checked = {'__file__': str(freezer)}
exec(freezer.read_text(encoding='utf-8').split('archive.mkdir()')[0], checked)
runtime = next(iter(checked['runtimes'].values()))
seconds = {stage: re.findall(r'test result: ok\..*?finished in ([\d.]+)s', checked['read'](stage))[-1]
           for stage in ['budget', 'worktree', 'dag', 'mixed']}
stamp = datetime.now().astimezone().isoformat()
body = '''088将生产OMP执行线程迁到原单写账本协调器：启动时把DurableWorkflow交给协调器，后台线程仅持有绑定项目、批准修订、任务、attempt和OMP适配器的客户端。原RPC、controller-request身份、文件工具、OmpJournal、产物封存、管理双ACK及恢复格式保持。等待模型、Git准备和管理确认不占写线程；执行结束先释放客户端，再归还原账本。没有第二份权威账本。

OMP事件沿独立受限命令通道入账，序号在原写线程分配；只能操作自己的分配、派发、开始、提交、未知恢复和原用户取消。API原Scope与Apply事务内容保持，API客户端不能取得OMP能力，OMP不能批准、Verify或Accept，也不能调用API派发入口。OMP证据路径仍由原账本为原attempt产生。

派发前在同一写线程复用原API请求计数：未封存API保留全部已批准轮数，CLI一次派发计1，Unknown不退还。实际Git预算夹具中，旧快照API/OMP同时争用最后一个额度只出现一次DispatchIntent；API保留3轮时OMP须另有1次余额。丢失入账回执或发布后中断均保留原派发意图，重开按原机制恢复Unknown、不重发；原API六项预算证据通过本批扩展回归。

新增三个协议/权限检查包含完成、取消、远端ACK丢失、供应商失败、缺失产物；实际prompt等待期间协调器可读且不能提前收回仍有客户端的账本。权限、跨attempt、API/OMP能力隔离和事件入账确认丢失均有拒绝断言。生产宿主原四项工作目录检查、依赖任务/重启/取消和混合执行者检查沿原路径运行，使用实际受控子进程和文件，不调用真实模型。首次夹具回调临时变量导致编译失败，改为命名观察器；初轮未用导入警告移除。失败日志保留，没有放宽生产约束。

边界：本批完成OMP执行内部的所有权迁移，RuntimeHost的OMP活动表仍按项目独占，Done仍归还DurableWorkflow；同项目OMP/API公开并行尚未开放，Codex仍待迁移。下一步将OMP协调器提升到共享宿主registry、按(project,attempt)归属活动与取消，并接原Startup前缀及管理双ACK，最后一个使用者退出才归还；不能仅删除独占保护。金额硬预算及OMP运行中监督仍按原规则明确拒绝，不因本批迁移声称支持。
'''
body += (
    '\n最终检查：456项默认Rust/165项显式忽略（核心300/146），5项Node、前端与核心构建通过；'
    f'预算8项{seconds["budget"]}秒、OMP工作目录4项{seconds["worktree"]}秒、'
    f'OMP依赖/恢复1项{seconds["dag"]}秒、混合宿主2项{seconds["mixed"]}秒通过。'
    '严格workspace全targets clippy与桌面custom-protocol debug构建通过；'
    f'三个runtime SHA-256一致：{runtime}。'
    '混合三种最终执行者各3任务Accepted、各派发一次、管理请求7次，中途/最终重开零新增事件；'
    '写后失败组合保留Unknown和文件、最终OMP零派发。\n\n'
    '原087的99份冻结归档与079的6份界面源文件摘要保持；原API生产派发、工作目录和独立验证器未修改。'
    '真实模型调用/用户桌面动作0，WorkBuddy064原单次编程授权已使用，不重发sum任务。'
    '075 Job退出观察及085投影共享冲突占用方未定结论继续保留。'
    '真实OpenAI管理、完整首批接入、真实异构编程和包含失败的效率、桌面可靠性、'
    'P6安装/升级/回滚/迁移/签名与支持矩阵仍需验收；完整P4/P5/P6未通过，目标active。\n'
)
latest = root / 'docs/HANDOFF-LATEST.md'
text = latest.read_text(encoding='utf-8-sig')
marker = '当前：2026-10-12 卷001第016项（087）'
assert marker in text and '088最终检查已通过' not in text
latest.write_text('# 最新交接\n\n当前：2026-10-12 卷001第017项（088），progress，088最终检查已通过。上一轮087为progress。\n\n'
    + stamp + '\n\n' + body + '\n[088验收索引](../reports/p5-omp-coordinator-20261012.json)。\n\n下方087为冻结历史。\n\n'
    + text[text.index(marker):], encoding='utf-8')
for name in ['docs/P5-WORKTREE-IMPLEMENTATION.md', 'docs/STAGE-ACCEPTANCE-2026-10-12.md',
             'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md']:
    path = root / name
    text = path.read_text(encoding='utf-8-sig')
    assert '088 OMP受限协调器' not in text
    if name.startswith('docs/'):
        text = text.replace('更新至批次087', '更新至批次088').replace('本页087小节', '本页088小节')
    prefix = '../../' if name.startswith('handoffs/') else '../'
    heading = '## 第017项 / 088 OMP受限协调器' if name.startswith('handoffs/') else '## 088 OMP受限协调器'
    path.write_text(text + '\n\n' + heading + '（本批检查通过，完整目标active）\n\n' + stamp + '\n\n' + body
        + '\n[088验收索引](' + prefix + 'reports/p5-omp-coordinator-20261012.json)。\n', encoding='utf-8')
path = root / 'docs/NEXT-STAGE-GATES.md'
text = path.read_text(encoding='utf-8-sig')
start = text.index('以下为较早阶段的历史记录。')
lead = '''# 后续阶段的验收执行顺序

2026-10-12 / 088 更新（本批检查通过）：先读[最新交接](HANDOFF-LATEST.md)、[当日验收矩阵](STAGE-ACCEPTANCE-2026-10-12.md)与[P5接入记录](P5-WORKTREE-IMPLEMENTATION.md)。完整P4/P5/P6仍未验收，旧记录保留当时边界。

- 084/085的API公开并行与自动运行、086共享独立验收、087共享主指挥Accept/依赖解锁、088 OMP内部受限执行与共同预算均已通过相应受控检查。复用原批准、管理双ACK、验证器和单写账本，不重做基础接入。
- 下一步将088内部持有的OMP协调器接入共享宿主registry，按(project,attempt)管理活动/取消；最后使用者退出才归还账本。当前OMP活动表仍按项目独占、Done归还DurableWorkflow，尚非公开OMP/API并行。保留原Startup持久前缀、双ACK与原始回执；再迁移Codex原native-turn、监督及取消所需受限能力，不能仅删除独占检查。
- 具体迁移点：api_host当前最后使用者判断只含API/共享验收，必须同时覆盖OMP；host快照不可继续拼接OMP旧缓存而重复同一项目。shared_verification_host必须等对应OMP线程退出才验收。codex_management_bridge的Prepare/Commit按原批准目标及历史前缀复核，先覆盖两次ACK窗口、原意图存在但尚未Assign、取消/断线和重开，不能将管理操作换成自动重发。
- OMP派发的原子预算已复用API原prior_requests：未封存API全轮数、CLI1次、Unknown保留。原API Scope/Apply命令权限未变，OMP专用客户端不具Verify/Accept/批准；主指挥087专用通道保持宿主持有。后续共享生命周期继续沿用此关口，不另建额度账本。
- 对照P3完整首批名单，按已有真实证据和实现差异补真实OpenAI管理及API/选定厂商CLI能力；模型ID以真实账号返回为准。旧Qoder/WorkBuddy原型成功保留，补产品版本/绑定/编程/可靠性缺口；WorkBuddy064原单次授权已使用，不重发旧sum任务。
- 真实串行与并行编程在共同规范、基准目录和独立验收下比较，完整记录失败/返工/管理成本/实际调用用量和人工时间。受控场景不能代替真实效率。
- P6保持干净目标安装/卸载、核心与桥接升级/失败恢复、备份/数据迁移、旧版本回滚拒绝不兼容新数据、签名与支持矩阵和全部首批承诺签收。debug构建和选择性源码归档不代证。

'''
path.write_text(lead + text[start:], encoding='utf-8')
print('088 handoff and next-stage gates updated; previous frozen evidence preserved.')
