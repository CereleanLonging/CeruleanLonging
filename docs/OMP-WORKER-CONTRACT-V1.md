# OMP 单请求执行通道与工作台身份

21:05 更新：OMP真实文件journal和可信工作流桥接已实现，保存派发/ACK/取消/终态及原始产物记录；恢复只核对原记录，不重发或重捕获产物。三个派发写入边界实际终止、离线RPC→提交→独立校验子进程→chief验收通过。工作区233项通过，独立验收扩展后4项桥接回归及最终严格clippy通过，详见 [OMP-JOURNAL-BRIDGE-V1.md](OMP-JOURNAL-BRIDGE-V1.md)。尚缺worker产品进程、RuntimeHost/UI、真实凭证与工具、真实模型故障矩阵；完整P0–P6未通过。以下为历史批次。


2026-10-09 20:03 +08:00 收尾。最终223项常规Rust测试通过（适配器49项），35项真实调用/专用夹具显式忽略；fmt、全工作区all-targets/custom-protocol严格clippy及前端构建通过。首次前端构建被沙箱以spawn EPERM拒绝，正常权限重跑通过，两份日志保留。最终真实只读回归 `reports/omp-worker-readonly-20261009-200119.json`：RPC2/get_state各一次，空闲、零工具/模型prompt，进程自然退出码0。该回归只验证共享传输的只读兼容，未执行模型任务。

本批承接 [真实只读探测](OMP-PRODUCT-INTEGRATION-V1.md)。执行/取消协议通过本地双向流替身验证；尚未用真实模型执行任务，也没有接入产品 worker 启动、凭证、工具或 RuntimeHost 调度。完整 P3/P6 未放行。

## 执行通道

`omp_transport::worker::WorkerTransport` 复用已验证的有界 RPC2 读取器，与只读接口分开。只读连接没有转换成 worker 或任意 RPC 的公共入口。一个 worker 连接只接受一次 prompt；预检后即占用该连接，超时、异常、journal 回调失败或 future 被丢弃都不能重发。

派发绑定 task/attempt/revision/fence、工作目录、原 session/provider/model、连接 ID、请求 ID 及提示正文 SHA-256。发送前重新读取空闲状态，当前版本要求工具目录为空；提示必须是有界普通任务文本，不允许斜杠命令。控制 RPC 使用独立保留命名空间。重试、模型 fallback、宿主工具和未支持交互一律拒绝。本批尚无工具代理，这个边界不是最终编程能力。

发送 prompt/abort 前调用可信本地持久化回调；回调报错则不发送对应命令。prompt ACK、abort ACK、请求终态分别报告，原始思考和消息正文不写入这些事件。当前测试回调是受控夹具，真实产品 journal 适配仍须实现；不能仅凭此接口声明崩溃恢复已完成。

完成仍要求正确 prompt ACK、相同请求 ID 的 prompt_result 和其后新的 get_state。后台工作未静止时有限轮询，模型或会话变化拒绝，agent_end 和取消 ACK 均不能冒充完成。完成/aborted/error 三种结果保持区分。已读取终态优先于迟来的取消；即使终态先于 ACK 到达，也要等待 ACK，期间不额外发 abort。

取消由唯一读取者处理，不新建 reader；半行和未完成分块留在原读取器中。abort 最多一次，发送前有独立回调。持续可读的消息流最多处理32帧即检查取消，并显式检查总执行截止时间。取消可发生在 prompt ACK 之前；关闭取消发送端不等于请求取消。发送前取消返回 NotStarted；发送意图之后但实际发送之前取消会追加单独 NotStarted 回调。这是本连接观察的未发送结果，未来核心必须持久化并显式处置，不能自动退预算或重用旧尝试。

## 持久身份与账本

`IdentityStrength` 新增 `controller-request`，内容为 `ControllerRequestIdentity { connectionId, requestId }`。这是控制器在独占连接中的关联，不是供应商原生 turn ID，也不承诺重连后的历史查询。旧 `provider-id` / `ui-observation` 字符串保持，旧字段与摘要格式未迁移；TypeScript 契约已重新生成。

新的身份仅允许 OMP；连接/请求 ID 按工作台标识规则检查，回执必须与原 Attempt 的整个 binding 一致，turn_id 必须为空。把请求 ID 填成原生 turn、改变连接/请求/会话/修订/围栏均不得接受。仅解码这些字段不授予运行权限，仍须既有批准和可信核心调用。

严格账本支持这种无原生 turn 的 Started/Submit/取消上下文，同时保持线程和原绑定校验。修正 `last_start` 和 NotStarted 门槛：以实际 Started/运行中对账事件为依据，不再把缺少 turn ID 误判成未启动。真实账本文件测试证明新身份可保存提交并重开；启动后进入 Unknown 不能被 NotStarted 回执退回可派发状态。未通过独立验证仍不能 Accept。

## 覆盖与下一步

新增执行测试覆盖终态先到、后台未静止、部分 JSON 下取消、ACK 前取消、持续消息下取消、丢 abort ACK、回调失败、future 丢弃、重复执行拒绝、错误关联、工具/重试拒绝、模型变化、自然完成与取消竞争。身份测试覆盖旧枚举编码、严格新字段、跨连接/请求篡改和伪造原生轮次。账本测试用合成 worker 回执与实际产物文件验证提交和重放，不冒充真实 OMP 编程。

直接后续工作：

1. 将 worker 通道接入拥有进程的产品客户端，固定批准的二进制摘要和独立 session/profile；只读探测中的占位凭证不得移作真实认证。
2. 将回调接入持久派发/ACK/取消/终态证据，并处理“取消先于 ACK”和“意图已写但未发送”的核心状态映射；重开只读核对，证据不足保持未知。
3. 接本机凭证生命周期、实际 provider/model 和用量；用可审核工具代理/系统边界实现文件和命令权限，随后接管理分配、产物捕获、独立验收及用户流程。
4. 用真实 OMP 验证编程、取消、错误/丢失响应、重启恢复及预算。原首批其他 API/CLI、Qoder/WorkBuddy 已有桌面会话、并行集成、效率与 P6 安装升级范围保持。

证据索引为 `reports/p3-omp-worker-review-20261009.json`。本批未重跑Node服务/IPC、原生界面、发布构建或安装；前端构建通过不等于安装包验收。既有验收详情偶发 ledger_recorded=false 本次通过，根因仍待查。独立测试及OMP进程均已结束。
