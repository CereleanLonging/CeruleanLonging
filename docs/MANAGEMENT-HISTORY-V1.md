# 管理历史分页与按需详情

2026-10-08。原 Snapshot 携带全部指挥问题、用户答复和放弃理由。合法长文本积累后会超过命名管道 4 MiB 响应限制，导致整个界面收不到结果。本批将这些历史正文移出常规快照。

## 读取契约

- 常规 Snapshot 只包含最新 50 条 ManagementSummary，每条消息预览最多 160 个字符（含省略号），不含问答及放弃理由全文。
- managementHistory 保存当前筛选总数、offset、nextOffset、筛选项目、全局证据 revision，以及每项目精确 total/pending/running/waiting。预算和规划锁依据精确统计，不按当前页长度推断。
- pending 指尚未应用或放弃的请求；waiting 单独表示已消费但未答复的 Wait。需要修改配置回答问题时，不把 waiting 当成禁止修改配置的锁。
- ReadManagementPage 支持项目筛选和固定 50 条分页。offset 大于零必须提供 revision，记录改变时整页拒绝并提示回首页刷新。排序按解析后的创建时间和操作 ID，避免字符串时间区和数字后缀排序错误。
- ReadManagementDetail 按 operation ID 返回单条完整记录。只有这次响应的 managementDetail 有值，后续常规 Snapshot 为 null；正文仍原样留在证据账本。

revision 由所有操作的 ID、当前状态及证据链头生成，包含放弃/答复等后续变化。分页游标属于单次查询参数，核心不保存跨用户或跨窗口共享的当前页。

## 权限与界面

两个读取命令在调度轮询之前处理，读取本身不消费管理回复、不写批准、也不触发派发；已授权后台运行仍由独立宿主时钟推进。选定最多 50 个 ID 后才构建摘要，避免先复制整个历史正文再截断。

界面保存当前筛选与页面，后台更新不会强行跳页。记录版本变化后显示刷新提示，旧页不能继续翻页或确认操作。完整正文按需加载；放弃或答复前重新读取详情并核对摘要中的 evidenceDigest，提交仍走核心现有权限和单条摘要校验。列表权限提示和历史 revision 都不授予额外执行权限。

保存处置后保留项目筛选、回首页刷新并重读该条详情。关闭确认不写记录；正文读取失败不会用空字符串伪装成已读原文。

## 验证与边界

新增常规测试覆盖 122 条长文本的分页无重复/无缺失、项目精确计数、完整正文不进入常规快照、旧 revision/缺失 revision/越界页拒绝、不存在详情拒绝，以及读取 Ready 规划不消费回复。另提供 1,000 条最大合法长文本的原生验收种子，所有模型数据为离线夹具，供应商调用为 0。

此修改解决管理记录的响应容量问题。其他全量 Snapshot 字段（项目、方案、执行产物等）的总体规模、更多于账本当前 1,000 次管理请求的归档策略和长期性能仍须按 P1/P6 测试；不能把本批通过视作所有大账本风险消失。已分配但未发送的 Attempt、未提交应用事务处置及原首批接入/发布范围继续保留。

原生证据和完成结论在测试结束后追加。

## 本机验收结果（14:06 +08:00）

常规回归 Rust97项、Node IPC4项、最终fmt/clippy、发布/调试构建通过，日志p1-management-history-checks/final-clippy/build/debug-build-20261008.log。首次严格检查发现测试辅助临时字符串的所有权写法不必要，已修正；旧clippy日志保留，最终检查通过。

reports/management-history-native-h0E43U/result.json：1000条最长合法问答/放弃文本、两个项目各500条，20页全量及两个项目分页无重复或缺失，精确计数、首尾详情原文哈希、默认快照不残留详情、原生下一页/筛选/完整记录通过。完整ServiceReply实测默认71019字节，最长单条详情响应215316字节，均低于4194304字节上限。详情message仍重复Wait原因，所以未声称每条≤128KiB。

原生明确放弃只新增一份.4用户证据，没有新增intent；旧revision续页、用全局revision冒充单条digest及重复旧摘要提交均被拒绝。停止整个测试核心再重开仍可读取全部记录，cleanup退出码0。本批真实供应商/worker模型调用0；耗时留在result.json，仅是本机样本，不作为跨硬件性能保证。

reports/management-review-native-Ghoaut/result.json回归实际放弃与Wait答复、按需正文及重启；p1-management-history-desktop-regression-20261008.log回归批准/规划精确统计/合成凭证。发布p1-management-history-release-20261008/release-launch.json正常关窗、核心可读，日常0新计划。分页条与答复对话框截图已目视检查。证据摘要reports/p1-management-history-review-20261008.json。完整P0–P6仍未放行。
