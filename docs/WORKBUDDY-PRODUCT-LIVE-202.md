> 当前状态：用户已批准并执行一次入口；因测试执行器未满足管理凭证可读取检查而在项目启动前失败。账本确认真实消息 0、模型调用 0、任务分配 0，原 live-intent 已保留。本计划不得重试；下方“等待授权”及命令均为原冻结计划的历史内容。正在修复并完整受控演练，后续真实执行须另建审阅范围再确认。原批准版本保存在 reports/workbuddy-product-202/approved-source。

# WorkBuddy 5.8.3 产品流程实机差异计划 202

状态：已完成只读准备与本地检查，等待新增一次真实发送授权。没有执行本计划的真实消息。

本计划沿用原型的接入、连续回复与取消证据，以及 199—201 的新版布局、读取、发送/停止适配；这次补充真实 WorkBuddy 与工作台持久调度、独立产物验收的组合证据。主/副指挥响应固定为测试数据，不调用 OpenAI，也不宣称完整三级链验收。

## 固定范围

| 项目 | 范围 |
| --- | --- |
| 应用与聊天 | 已打开的 WorkBuddy 5.8.3.0，原“工作台接入验证”，Hy3 |
| 身份 | 原 PID 17252、HWND 658014、启动时间及 exe/asar 摘要；同时检查文档、历史、布局、空草稿与模型 |
| 消息上限 | 最多 1 次发送尝试；无建议、无停止操作、无自动重发 |
| 唯一请求产物 | `D:\test\CeruleanLonging\reports\workbuddy-product-202\prepared-final\project\sum.mjs` |
| WorkBuddy 任务 | 导出 `sum(a,b)`；两个参数均为有限 number 时求和，否则抛出 TypeError；简短说明并输出本次关联标记 |
| 对 WorkBuddy 的要求 | 只写上述文件，不改其他文件、不运行命令、不联网、不读凭证；目录不适用或需要额外授权时停止并说明 |
| 独立验收 | 工作台运行已登记 Node 校验器，权限仅允许读取验证器和产物；VM 内 15 项数值/类型检查 |
| 等待界限 | 任务策略预算 90 秒，原生回复观察至多 50 秒，执行器总时限 210 秒；这些是本地界限，不保证外部应用已停止推理 |
| 计费 | 一条桌面任务可能包含多轮供应商推理；会消耗 WorkBuddy 额度，不声明金额硬上限 |

当前实际工作区是否允许访问目标目录仍由 WorkBuddy 的权限和配置决定。隔离目录是本次请求与验收范围，不是对外部应用施加的文件 ACL；发送授权不包含替它确认额外权限弹窗。

## 已准备的执行链

1. 使用生产桥接再次只读检查原聊天，核对冻结的执行器、桥接、应用、Node、验证器和计划摘要。
2. 以 create-new 方式持久写入唯一 `live-intent.json`。存在该文件即拒绝再次启动本计划。
3. 在隔离 RuntimeHost 中批准已冻结的方案，由固定副指挥响应分配原任务。
4. 生产逻辑生成 TaskReview，保存完整消息后仅调用一次发送。消息中的 attempt ID 和 `CL_TASK` 标记在此时生成，其余内容由冻结方案和生产消息模板决定。
5. 生产桥接读取原回复，工作台独立验证产物，固定主指挥响应只对已独立验证的结果执行 Accept。
6. 成功后重开工作台数据两次，核对任务与管理账本无新派发。所有进程由已有 Job 包装器收尾，用户 WorkBuddy 保持运行。

失败、未知、超时或身份变化即停止发送流程，保存原记录；后续只能按原任务只读核对，不因超时自动重发、停止或追加消息。本轮不验证取消或桌面原任务插话。

## 冻结材料与命令

- [正式审阅 JSON](../reports/workbuddy-product-202/prepared-final/review.json)
- [审阅摘要](../reports/workbuddy-product-202/prepared-final/review-digest.json)：`50d14cf0936605667f7cf456f6eb34bc7c017416c19bf9bf47185658c8919597`
- [最终只读准备回执](../reports/workbuddy-product-202/prepare-final-owned/cleanup.json)
- 执行入口：[test-workbuddy-product-live.ps1](../scripts/test-workbuddy-product-live.ps1)
- 实现：[workbuddy_product_live.rs](../crates/core/src/workbuddy_product_live.rs)

仅在用户明确批准上述范围后执行一次：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/test-workbuddy-product-live.ps1 -Live -PreparedDirectory D:/test/CeruleanLonging/reports/workbuddy-product-202/prepared-final -EvidenceDirectory D:/test/CeruleanLonging/reports/workbuddy-product-202/live-owned -TestExecutable D:/test/CeruleanLonging/target/debug/deps/cerulean_core-311e0366bb05daa9.exe -ApprovedDigest 50d14cf0936605667f7cf456f6eb34bc7c017416c19bf9bf47185658c8919597
```

当前没有 `live-intent.json`；不使用 earlier `prepared` 或 `prepared-visible`。第一次只读准备因窗口最小化失败；恢复原窗口后准备通过。中间计划的 Node 权限参数使用扩展 DOS 路径而被拒绝，已修正为同一文件的普通路径并通过 15 项既有产物检查，最终计划已重新生成。旧日志保留。

新增确认来自之前有限次数 WorkBuddy 消息授权已经消费，且包含失败即停、不自动重发约定。本计划是新的编程任务与额度消耗；OMP 的一次性 Key 授权不适用于本计划，该 Key 已清理，不会复用。

即使本计划通过，真实 OpenAI 管理链、取消/恢复实机差异、现有桌面严格原任务监督、完整供应商矩阵、当前候选发布流程、独立 Windows 与正式签名仍需分别验收。
