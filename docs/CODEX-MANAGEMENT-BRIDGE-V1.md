# 副指挥分配与 Codex 派发桥接

2026-10-08。本批把已保存的副指挥 Assign 决定接入真实 Codex 会话准备、分配与单次派发。一次真实编程和独立验收通过；副指挥回复及用户批准仍来自测试夹具，完整 P2 尚未通过。

## 路径与持久化

`RuntimeHost::start_managed_codex(operation, attempt)` 从保存的副指挥回复读取项目、任务与 worker，调用者只能指定新的本地 Attempt ID。核心重新校验状态摘要、实际批准、worker 模型/程序、任务依赖和执行限额。已有应用事务或该决定已经关联过 Codex Job 时拒绝重新启动，即使更换 Attempt ID。

Codex Job 增加可选 managementOperation，旧记录缺省为空。真实 worker 持有项目账本，核心持有指挥账本，两者通过内部通道进行两次确认：

1. worker 连接 Codex 并创建实际会话，构造包含真实 session ID、版本、工作目录和 fence 的 Attempt。尚未发送任务提示。
2. 核心核对活动 Job、原副指挥决定、当前快照与目录，保存应用意图后确认。worker 才提交固定 `mgmt-<operation>` 的 Assign。
3. worker 提交实际 Entry 和新快照。核心核对角色、命令、上下文、序列及磁盘上的不可变事件，保存引用该事件哈希的 Applied 回执，再确认。
4. worker 收到第二次确认后，进入原有 DispatchIntent → 单次 turn/start → Started → Submit 路径。没有第二条旧式 Assign。

每次确认最多等待 30 秒。任一确认丢失、超时或拒绝，都结束当前路径，不自动重发。第一次失败不会形成 Assign；第二次失败可能已形成 Assign 或 Applied，但不会因此绕过确认发送提示。项目会保守停留在待处置状态。Assigned 而尚无 DispatchIntent 的显式恢复/放弃入口仍待实现。

同项目仍由常驻 worker 独占账本，核心查询保持可用；新的桥接使用既有服务轮询。桥接运行期间暂不允许变更全局验收目录，避免决定上下文变化。旧 `start_codex` 内部路径保留兼容测试与迁移；正式编排应使用新入口，不能先独立应用 Assign 再调用旧入口。

重开时，Codex Job 必须与指挥请求项目/用途、任务、Attempt 和实际工作流事件对应；管理分配事件不能通过移除 Job 的 managementOperation 退回旧路线，同一决定不能关联多个 Job。指挥回执丢失仍沿既有工作流对账补齐。未完成事件保持未知，不启动模型请求。

## 证据

新增两项常规测试，内部多种故障场景覆盖两次确认成功、第一次确认丢失、第二次确认丢失、上下文变化、声称已提交但与磁盘不一致的事件。确认失败均未越过发送前的返回点；这些是实际线程/文件/通道测试，传输仍为替身。

常规 Rust **83 项**、Node **4 项**通过，见 `reports/p2-managed-codex-full-checks-20261008.log`；严格检查见 `p2-managed-codex-clippy-20261008.log`。新增一个必须显式运行的真实测试，常规回归不会调用它。

真实报告：`reports/p2-managed-codex-live-20261008-035625/result.json`。Codex 0.160.1、现有登录、gpt-6.1-sol，一次实际编程生成 `project/sum.mjs`；独立 Node 验收程序运行 10 项数值断言。实际账本仅一条 Assign、一条 DispatchIntent，分配事件与应用回执哈希相同。测试共 1388 次宿主轮询，不是客户端重连数，也不是大规模性能指标。原会话 `01a117f0-a144-7483-b301-ee91f7dfdd43`。

重开后保持 Submitted 且 verified=true，原副指挥决定为 Applied，更换 Attempt 再提交也被拒绝，重启新增模型发送 0 次。本批 worker 模型调用 1 次，主副指挥模型调用 0 次，没有 chief Accept。实际供应商身份与完整主副指挥链不能由这个夹具证明。

首次编译的序列类型及测试缺少验收参数错误已修复，初始日志保留。发布构建与启动结果见本批交接及汇总报告。

## 下一步

接入真实需求录入、可执行 worker/validator 与主副指挥模型/预算配置，把“主指挥方案 → 实际用户批准 → 副指挥决定 → 新桥接 → 独立验收 → 主指挥收口”串成产品流程。未知请求和未派发 Attempt 的显式处置、活动取消/监督建议、diff、崩溃和全部 P1/P3–P6 门槛仍需完成。真实官方 API Key 仍待用户本机配置；不从其他应用读取凭证。
