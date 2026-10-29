"""Update the existing 086 handoff after checking the final evidence."""
from pathlib import Path
from datetime import datetime

root = Path(__file__).resolve().parents[1]
freezer = root / 'scripts/finalize-shared-verification-20261012.py'
checked = {'__file__': str(freezer)}
exec(freezer.read_text(encoding='utf-8').split('archive.mkdir()')[0], checked)
runtime_hash = next(iter(checked['runtimes'].values()))
summary = (
    '086最终检查通过：453项默认Rust/161项显式忽略（核心297/142），5项Node、前端与核心构建通过。'
    '自动并行3项4场景111.38秒、共享验收1项5场景115.44秒、API工作目录8项120.37秒、'
    '原预算6项68.66秒、混合宿主2项121.95秒通过。自动并行沿用详情状态修复前已通过的最终记录；'
    '修复后全量和新增详情/生命周期专项通过，没有重复旧接入。严格workspace全targets clippy与'
    '桌面custom-protocol debug构建通过；三个runtime SHA-256一致：' + runtime_hash + '。\n\n'
    '原085的107份冻结归档及079的6份界面源文件摘要保持，执行器Scope/Apply事务与API命令权限未扩大。'
    '原失败日志全部保留。混合三种最终执行者各3任务Accepted、各派发一次、管理请求7次；'
    '中途和最终重开零新增事件，失败组合保留Unknown与文件、最终OMP零派发。'
    '真实模型调用/用户桌面动作0。完整P4/P5/P6仍未验收；下一步共享主指挥Accept和Codex/OMP生命周期，'
    '之后真实模型编程/含失败的效率与完整安装、升级、回滚、迁移、签名。WorkBuddy064原单次任务不重发。'
)
paths = [
    'docs/HANDOFF-LATEST.md', 'docs/P5-WORKTREE-IMPLEMENTATION.md',
    'docs/STAGE-ACCEPTANCE-2026-10-12.md',
    'handoffs/2026-10-12/handoff_2026-10-12_001_000100+0800.md',
]
stamp = datetime.now().astimezone().isoformat()
for name in paths:
    path = root / name
    text = path.read_text(encoding='utf-8-sig')
    assert '086最终检查通过：' not in text, name
    text = text.replace('最终检查进行中', '最终检查已通过')
    text = text.replace('随后补入详情只读检查，最终回归进行中。', '随后补入详情只读检查，最终回归结果见本节收尾。')
    text = text.replace('最终全量、生命周期/详情、worktree、原预算、混合宿主与桌面构建尚在进行。',
                        '最终全量、生命周期/详情、worktree、原预算、混合宿主与桌面构建均已通过，见本节收尾。')
    link_prefix = '../../' if name.startswith('handoffs/') else '../'
    final = stamp + ' 收尾：' + summary + '\n\n[086验收索引](' + link_prefix + 'reports/p5-shared-verification-20261012.json)。\n'
    if name == 'docs/HANDOFF-LATEST.md':
        marker = '冻结索引预定 reports/p5-shared-verification-20261012.json；下方085为历史边界。'
        assert marker in text
        text = text.replace(marker, final + '\n下方085为历史边界。')
    else:
        text += '\n\n' + final
    path.write_text(text, encoding='utf-8')
path = root / 'docs/NEXT-STAGE-GATES.md'
text = path.read_text(encoding='utf-8-sig')
text = text.replace('最终检查进行中', '最终检查已通过').replace('当前正在最终检查。', '本批最终检查已通过，详见086冻结索引。')
path.write_text(text, encoding='utf-8')
print('086 current handoff, acceptance matrix and next gates finalized; prior frozen evidence unchanged.')
