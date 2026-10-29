# Codex 派发与恢复：执行组件验收

2026-10-08，02:16 批次记录。正式核心 `crates/core/src/codex_execution.rs` 已完成一次真实编程执行与独立验收，以及一次针对同一真实轮次的只读恢复。随后已接入常驻宿主，最新边界见 [常驻执行](CODEX-RESIDENT-V1.md)。**P2 整体未通过**：使用预先批准的集成测试方案，不能替代真实主指挥提案、用户操作批准、副指挥分派与主指挥收口。

## 已实现的执行顺序

1. 核验完整方案批准、策略版本、Codex 适配器、任务与 Attempt、工作目录和会话绑定；要求批准读取、写入与命令执行权限。
2. 核验请求次数和时间限制；先持久化 DispatchIntent，再唯一一次发送 turn/start，并传入 Attempt ID 作为 clientUserMessageId。
3. 收到真实 turn ID 后写 Started。完成通知必须匹配 thread/turn 且状态为 completed；再用 thread/read 读回核验。
4. 读回要求同一规范工作目录、会话、唯一匹配的用户消息 clientId、完整文本提示及已知轮次。缺失、重复、部分视图或不匹配均拒绝。
5. 核心独立采集产物摘要并写 Submitted；独立 runner 使用已批准的工具和受测副本写 Verify。Submitted/verified 不等于 Accepted，主指挥收口仍待接入。

错误或截止时间到达会使通道失效，任务尽可能入账为 Unknown；不会自动重发。恢复使用新连接读取原会话，不 resume、不发送新的模型提示。仅找到唯一且已完成的原轮次才可写 Reconcile Submitted；缺失结果不能推断为“从未开始”。

本客户端持久化身份、状态、提示摘要和项目类型，不保存原始推理正文。官方协议之外的关键字段已对照本机 Codex 0.160.1 生成的 JSON Schema，真实读回确认 userMessage.clientId 可与发送标记关联。

## 本次真实证据

目录：`reports/p2-live-dispatch-20261008-020440/`。

| 项目 | 实际结果 |
| --- | --- |
| 执行器 | 本机 Codex 0.160.1，既有 ChatGPT 登录，服务器默认 gpt-6.1-sol |
| 真实任务 | 生成无依赖 ES 模块 sum.mjs；校验有限数字数组，不改 result.txt |
| 模型发送 | 1 次 turn/start；不代表服务端内部只有一次推理请求 |
| 独立检查 | 外部固定 Node 程序和校验脚本，10 项断言通过 |
| 原工作目录 | result.txt 保持 abc，仅新增 sum.mjs；未以模型口头报告判定通过 |
| 原始报告 | result.json、test.log、approved-fixture-plan.json、ledger/ |
| 重开结果 | Submitted，verified=true；没有伪造 Accept |
| 恢复 | recovery-result.json、recovery-final-test.log、recovery-ledger-with-bindings/ |
| 恢复模型发送 | 0 次；读回同一原始 turn ID，再独立验收 |

原始会话 `01a1178a-09a3-7d83-bba6-11f3f9ab0e20`；轮次 `01a1178a-0b40-7d40-b875-7440152376be`。恢复通过复制真实账本至 DispatchIntent 为止来模拟丢失 ACK，保留原账本。这是故障注入测试，不宣称实际发生了网络掉线或主机崩溃。

第一次恢复夹具漏复制 bindings.json，在访问模型服务前失败；修复夹具后换用新目录通过。旧 recovery-ledger/ 与 recovery-test.log 保留。编译时预算参数 u64/u32 不匹配也已修正；失败日志与最终通过日志分别保留。

## 明确限制及下一步

- 本批当时为生产核心库加集成验证入口；后续已接 RuntimeHost、持久化 worker 描述、可信分派回调、异步状态和原生只读恢复，见 CODEX-RESIDENT-V1.md。活动取消和真实指挥链仍待完成。
- 主副指挥的真实身份、独立权限和模型决策尚未验收；provider=openai 及 ChatGPT 登录类型不足以证明这些条件。
- 订阅路线没有可核验美元费用，因此拒绝带金额预算的派发，不假装已执行金额上限。max_requests 限制工作台发送次数，不限制 Codex 内部调用；当前计数保守地覆盖整本账本，未按方案版本重置。
- 异步执行受总时限限制，但同步磁盘操作不能被该时限硬中断。关闭客户端或子进程不等于远端任务已经取消；进程树、真实中断和宿主崩溃测试仍需完成。
- 返回的 workspaceWrite/禁网配置不是完整 OS 隔离验收。动态依赖、工具子进程、目录竞争及最终集成租约仍须测试。
- 当前读回仅恢复已完成轮次；活动轮次续接、流式 UI、失败任务的修复审批和最终 chief 收口尚未通过。

后续全阶段门槛见 [NEXT-STAGE-GATES.md](NEXT-STAGE-GATES.md)。测试证据被 Git 忽略，交接必须另存 reports；不得为了重跑删除旧账本或对 Unknown 自动换 ID 派发。
