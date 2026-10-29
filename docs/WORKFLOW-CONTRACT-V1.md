# 调度领域内核 v1：迁移契约与验收边界

2026-10-07。本次固定的是原型控制面在 Rust 中的迁移子集，不代表完整产品契约已冻结。实现位于 `crates/domain/src/workflow.rs`，没有网络、模型调用、进程执行或文件写入。

## 已实现的契约

| 对象 | 字段及约束 |
| --- | --- |
| Principal | id、role、provider；主副指挥 provider 必须为 openai；ID 不重复；由可信宿主绑定 |
| WorkflowPlan | id、version、tasks；任务图无环、依赖存在；任务 ID/工作区键/资源键合法；版本推进后批准失效 |
| WorkflowTask | id、dependencies、workspaceKey、goal、可选 resourceKeys；资源集合随分配一并检查 |
| TaskRun | state、worker、attempt、fence、evidence、verified、可选 cancellationRequested |
| WorkflowEvent | 连续 sequence、principal、command；重放逐条验证权限与转换 |
| WorkflowState | 当前计划、摘要、批准状态、任务状态、监督建议 |

命令：propose、approve、assign、dispatch-intent、started、cancel-intent、recovery-unknown、reconcile、submit、verify、accept、advise。

常规转换：`pending → assigned → dispatching → running → submitted → accepted`。`verified` 保持独立布尔字段，与原型相同；仅通过独立验证的 submitted 可由 chief 接受。不要在前端自行把 submitted 当作完成验收。

恢复时 dispatching/running/cancelling 转为 unknown。unknown 保留工作区及显式资源占用，不能直接派发；只有可信核心持有证据的对账允许转至 assigned/running/submitted/cancelled。取消意图已登记时，不能借 not-started 重新派发。自然完成与取消终态分别记录。

副指挥只能分配与建议，不能自批、执行、验证或收口。worker 只能提交其绑定的当前 attempt；core 验证，chief 收口。所有拒绝转换保持内存状态与事件列表不变。

## 兼容规则

工作流计划摘要沿用原型规范化后的 JSON 字段顺序。resourceKeys 缺省与显式空数组不同，不擅自归一化，以保持已有批准摘要。恢复事件按 JS 对象键顺序：数组索引形式的数字 ID 先升序，其他 ID 依计划插入顺序。

基础桌面的 `Plan` / `Command` 仍为手工计划保存与批准接口，摘要是另一格式，不能直接作为 WorkflowPlan 的已批准凭证。新的 WorkflowCommand 尚未暴露给 Tauri 调用方；前端不能传入任意 principal 获得核心权限。接入时必须由宿主验证通道身份并生成命令。

本日后续已在 `execution.rs` 加入 Project、策略/预算/工具范围、Attempt、SessionBinding、结构化回执/产物/验收报告和能力三态，详见 [执行契约](EXECUTION-CONTRACTS-V1.md)。这些契约尚须持久化并连接认证与执行入口，P0 仍未整体验收。

## 对照证据

原型源：`D:\test\workbench-probes\src\workflow-core.ts`。生成器 `scripts/generate-workflow-fixtures.mjs` 在可信控制面上执行场景，保存每一步的成功/拒绝、状态与完整事件序列；不是用 Rust 自己生成自己的期望结果。

冻结夹具：`tests/fixtures/workflow-parity-v1.json`，12 组、135 步、29 次预期拒绝，覆盖批准/版本、权限、依赖、工作区、共享资源、状态未知、取消、错误执行者、过期尝试、失败验证、图校验与恢复顺序。Rust 测试逐步比较状态、日志和摘要，并检查拒绝原子性。另有非法指挥绑定、重复身份和日志序号缺失的测试。

初次对照未通过数字 ID 的恢复事件顺序；已修正 Rust 顺序并保留案例，未修改原型期望来迁就实现。结论仅为这些控制面用例一致，不等于所有输入或真实故障迁移已完成。

普通 CI 读取仓库中的冻结夹具，不依赖相邻原型目录。只有更新基线时才运行生成器；更新须审阅差异及原型源 SHA-256，不能为了把失败变绿盲目再生成。

## 下一道实际放行门槛

持久化宿主必须先校验并提交派发/取消意图，才允许副作用；恢复新增的 unknown 事件也必须先落盘。本日已新增持久化库封装、四个落盘点/三个派发点的进程测试及每日交接投影，见 [持久化契约](DURABLE-WORKFLOW-V1.md)。纯领域内核仍不执行副作用；下一步是完整身份/证据契约、常驻服务及用户入口，之后再启用真实适配器。
