# Codex 正式适配器：连接、工作会话与轮次读回

2026-10-08。正式代码位于 crates/adapters/src/codex.rs。握手、现有登录、模型发现、受限工作会话、真实编程轮次及只读恢复已实测；派发核心位于 crates/core/src/codex_execution.rs。产品完整业务闭环仍未完成，详见 [执行组件验收](CODEX-DISPATCH-V1.md)。

## 已进入产品的路径

“接入管理 → Codex Agent”填写本机 codex.exe 的完整路径，点击检查。核心以独立线程启动 app-server，读握手、account/read 的登录类型和 model/list。界面轮询更新，检查不会创建计划或发送模型提示。账户邮件、凭证及配置正文不进入 UI/报告；本客户端不保存原始推理正文。

检查状态只保留本次核心进程内的观察，重启核心后须重查；关闭桌面而核心仍运行时仍可读取。检查运行时核心正常退出被拒绝，避免中途丢失结果。客户端完成后关闭自己的 app-server 子进程，既有 Codex 应用及会话不退出。

核心同时提供诊断命令：

```powershell
cerulean-core.exe --codex-check --executable <codex.exe绝对路径> --workspace <隔离目录绝对路径>
cerulean-core.exe --codex-bind-check --executable <codex.exe绝对路径> --workspace <隔离目录绝对路径>
```

第二条还创建工作会话并验证返回的目录、模型、provider、workspaceWrite、networkAccess=false、临时目录排除和 approvalPolicy=never。模型取服务器目录中的默认项，不硬编码“最新型号”。此命令只建会话，不发送 turn/start。配置中的命名 MCP/插件在内存读取后关闭，不输出其配置值。

## 协议和执行边界

使用 Tokio 子进程标准输入输出，不依赖产品 Node 运行时。每帧最多 4 MiB，每个请求最多 64 KiB。写入和观察均有截止时间；坏 JSON、错误回复 ID、RPC 错误、超时将通道标为不确定，不重发。只保留有界终态队列，完成事件早于 turn/start 回复也能按 thread/turn 关联。服务端额外命令/文件审批请求被拒绝，其他工具或凭证请求返回不支持。

可信宿主可使用 create_worker、start_turn、wait_turn、interrupt。start_turn 要求同一个已确认绑定并保留已发出状态；ACK 丢失或缺失轮次身份后不能再次发送。调用方必须先写 dispatch-intent，已知 turn ID 再入账 started；只有 completed 才能进入独立验收。failed/interrupted 与断线不能算成功。

**本轮新增：**正式派发库已串联批准、DispatchIntent、Started、真实 completed/读回、产物与 Submitted，再由独立运行器 Verify。thread/read 按 clientId、提示摘要和唯一原轮次对账；真实只读恢复没有重发。集成测试预置方案与批准，不构成真实主副指挥验收。

**常驻执行进展：**RuntimeHost 已持有 worker 生命周期并接入受信派发回调；桌面显示后台状态并可只读核对原会话，模型/程序摘要进入方案批准。见 [常驻执行](CODEX-RESIDENT-V1.md)。尚待真实主副指挥结构化决策、用户全流程、活动轮次 resume、流式输出、真实取消、目录策略实效、进程树清理与最终收口。当前关闭子进程不构成远端任务已取消的证明。

返回 provider=openai 和 ChatGPT 登录类型尚不足以证明主副指挥的完整上游身份及工具隔离；主副指挥的独立权限认证保留为 P2 门槛。现有工作线程属于执行者路线。

## 本机实际证据

- 最新 reports/p2-host-codex-final-checks-20261008.log：Rust 63 项、Node 4 项。常驻宿主真实执行及恢复见 p2-resident-codex-20261008-023403/result.json；本批编程发送 1 次，恢复新增 0 次。
- 历史 reports/p2-dispatch-full-checks-20261008.log：Rust 61 项、Node 4 项；两项真实集成测试在 p2-live-dispatch-20261008-020440/。该批编程发送 1 次，恢复新增 0 次。以下为更早的连接批次，不能将其 modelCalls=0 沿用到当前批次。
- reports/p2-codex-live-connection-20261008.json：Codex 0.160.1、ChatGPT 登录、8 个可用模型，modelCalls=0。
- reports/p2-codex-live-binding-20261008.json：同一正式 Rust 适配器确认限定测试目录的真实工作会话，模型由返回目录选定，modelCalls=0；创建的会话 ID 保留用于审计，未重复派发。
- reports/p2-codex-full-checks-20261008.log：Rust 57 项、Node 4 项；包括提前终态、拒绝审批、错误 ID、坏帧、超时不重发、帧限制及阻塞写入边界。
- reports/p2-codex-final-clippy-20261008.log：全 workspace/all-targets 严格检查。
- 最终原生页面及发布版本报告见最新交接；构建成功不能替代真实业务执行。

## 协议依据

核对了 [OpenAI 官方 App Server 文档](https://learn.chatgpt.com/docs/app-server) 中 initialize、account/read、model/list、thread/start、turn/start、turn/completed 和审批请求规则；实际安装版本 0.160.1 的返回仍由代码核验。官方协议为可演进接口，升级后需重验。

还核对了 [Responses 迁移说明](https://developers.openai.com/api/docs/guides/migrate-to-responses) 与 [结构化输出](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses)，作为后续官方 Key 指挥路线的依据；本轮没有调用该路线，也没有把 Codex 订阅登录转换成 API Key。
