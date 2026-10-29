# 协作改动核查与纠正 / 166 / 2026-10-15

本轮核查当前源码、161–165 冻结源码索引、166 实际执行回执及现有发布产物。结论：发现并修复一处实际界面缺陷；一处验收状态命名需补正；一项性能验收边界必须保留。没有依据否定其他模型的全部成果，也没有发现这些批次把完整正式验收标记为通过。

## 1. 已修复：活动运行把历史页拉回首页

161 的 `WorkspaceProjectRuns.tsx` 在 `activeCount > 0` 时轮询 offset=0，没有排除用户正在浏览历史页的情况。活动任务与分页同时出现时，一秒轮询会覆盖第二页。原检查分别覆盖分页与轮询，遗漏了两者组合。

166 新增回归先在原实现上复现失败（`active polling must not evict a pinned history page`，实际请求数 6、预期 5），再把自动轮询限制为首页，并在历史页提示暂停自动刷新。回到首页后恢复轮询。新生产组件受控 UI 检查 13 组通过，保留既有错误、迟到响应、并发锁及详情失效行为。

- 修复前失败与进程清理：[失败记录](../reports/workspace-project-runs-ui-owned-166-de20d378049542619209a5d4a2052e8b/cleanup.json)、[断言](../reports/workspace-project-runs-ui-owned-166-de20d378049542619209a5d4a2052e8b/stderr.log)。
- 修复后：[13 组结果](../reports/workspace-project-runs-ui-166-lIFykU/result.json)、[进程清理](../reports/workspace-project-runs-ui-owned-166-0321dba2d0e642fd84f8c7cef99cef6e/cleanup.json)。
- 原 161 报告、测试与源码快照不改写；本轮新增 166 回归文件。

## 2. 补正：165 的全局查询状态名

165 结果的 `kind`、`scope` 和正文已正确说明 API publication review 是全局集合，保留无项目归属记录；但 `status` 使用 `accepted-limited-selected-project-api-publication-review-bounded-read`，其中 selected-project 会误导汇总读取方。

后续消费该结果应按 **accepted-limited-global-api-publication-review-bounded-read** 解读，仅表示该全局只读切片的限定通过。原冻结 JSON 不修改，本条作为解释性补正；它不增加项目过滤、不改变原记录身份，也不改变 releaseAccepted=false。

## 3. 未关闭的性能边界

`workspace_management_decisions.rs` 先收集选中项目全部状态，再逐项构造 `management_detail` 放入 `records`，最后分页。返回每页最多 20 项、约 64 KiB、完整编码结果不超过 512 KiB，只证明响应有界；内部读取、详情构建、内存与耗时仍随历史规模增长。

这是静态代码确认的实现限制，本轮没有测得性能故障，不应写成“已经卡顿”或“性能验收完成”。新 166 管理复核集合也会重新读取、验证整个原凭据集合，不能宣称常量时间/内存。后续应在保持原证据链及版本一致性的前提下，设计增量索引或流式筛选，再测代表性数据规模下的峰值内存与延迟。不得为优化略过原凭据校验。

## 4. 保留的有效成果与证据范围

155 对作业原始记录和动态恢复投影的区分是有效改进；旧设计的问题也包括本会话前序工作，不归责于其他模型。161–165 共 92 份独立源码/依赖补丁快照的 SHA256 与索引一致；这证明归档一致，不等于逐项重跑或验过所有产品行为。[核查数据](../reports/workspace-management-record-reviews-166/verification.json)。

166 同时完成 165 已列出的下一项：全局管理残片复核记录的独立分页/原详情读取，保留原可选 project_id；CLI、原 workspace_read 与 UI 接入。不改变原复核、执行、批准守卫。原 `management_review.rs`、`management_execution.rs`、`management_review_tests.rs` 与入场摘要一致。

专项 Rust 2/2、CLI 4/4、原管理复核 7 通过/2 个既有隔离崩溃测试 ignored；原过滤器实际命中 9 项，并非空跑。全局管理 UI 15 组通过；这是受控组件测试与本地持久化夹具，不是真实模型、安装版或桌面接管验收。契约生成、最终 TypeScript、Vite、严格 workspace/all-targets Clippy 与 fmt 通过。Vite 大 chunk 提示保留。UI 包装器被 Windows PowerShell 执行策略拒绝及 Vite 沙箱 spawn EPERM 的失败日志保留；通过现有宿主执行完成，没有调整执行策略。

## 5. 当前签名与版本一致性

重新读取当前 52 个原产物、11 份清单及公开证书元数据；本次实读摘要全部与原离线审计一致，原发现范围内没有新增产物/清单，代码签名证书指纹和相关信任位置集合一致。[当前回查](../reports/signature-close-consistency-166-20261015.json)。因此只对这些逐字节相同的文件继承原离线签名结论，没有直接照搬昨天的版本判断。

当前仍存在名称为 `CeruleanLonging Local Test Only` 的本地测试证书；存在证书不代表当前构建产物已签名，也不代表正式可信发布。此次没有重新运行 WinVerifyTrust、联网吊销核验、使用或新建密钥、签名、变更信任。当前新增源码尚未制成并验收新的安装候选；旧产物的一致性不证明它包含 166 的代码。

## 后续优先级

先把已积累的源码切片落实到版本/清单/reader 一致的候选安装产物，再完成对应 Tauri/core 集成与原门槛清单。正式签名更新、独立 Windows 安装、原用量预算、活动监督恢复、真实供应商/两桌面矩阵及多任务效率等完整 P3–P6 尚未通过；不因只读分页切片数量增加而提高正式验收状态。原型通过的基础接入继续复用，只补实现差异和缺失证据。

target 清理成果保留；本轮只读统计为 50.58 GiB（此前 49.12 GiB 后产生了构建文件），`.cargo/config.toml` 仍关闭增量缓存。本轮没有删除他人构建或冻结产物。
