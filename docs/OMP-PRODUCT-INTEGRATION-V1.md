# OMP 产品接入：协议、进程探测与后续实现

21:24 更新：OMP Windows受控进程已实现，创建时原子加入Job、固定二进制摘要并锁定文件、隔离profile与显式密钥环境。真实OMP握手/自然退出回归零模型prompt、剩余进程0；三个显式离线进程/宿主终止测试及233项工作区测试通过，严格clippy通过。见 [OMP-OWNED-PROCESS-V1.md](OMP-OWNED-PROCESS-V1.md)。尚待RuntimeHost/UI、产品凭证选择、审核工具代理和真实编程；完整P0–P6未通过。以下为历史批次。


21:05 更新：OMP真实文件journal和可信工作流桥接已实现，保存派发/ACK/取消/终态及原始产物记录；恢复只核对原记录，不重发或重捕获产物。三个派发写入边界实际终止、离线RPC→提交→独立校验子进程→chief验收通过。工作区233项通过，独立验收扩展后4项桥接回归及最终严格clippy通过，详见 [OMP-JOURNAL-BRIDGE-V1.md](OMP-JOURNAL-BRIDGE-V1.md)。尚缺worker产品进程、RuntimeHost/UI、真实凭证与工具、真实模型故障矩阵；完整P0–P6未通过。以下为历史批次。


20:03 更新：单请求worker协议、取消竞争、controller-request身份和严格账本无原生轮次的启动/提交/重放已通过组件验证，见 [OMP-WORKER-CONTRACT-V1.md](OMP-WORKER-CONTRACT-V1.md)。最终223项常规Rust、严格检查和前端构建通过；真实只读入口回归通过。worker进程、产品journal回调、凭证、工具和调度尚未贯通，下一步按该文档四项继续。以下为历史只读批次。

## 2026-10-09 14:48：真实进程只读探测通过

`omp_transport` 已实现有界 JSONL、RPC2 Base64 分块重组、单读取者、严格请求 ID/command 关联和一次协商。物理行包括换行最多 1 MiB，逻辑消息最多 64 MiB；乱序、插入其他消息、长度不符、非法编码、未协商分块及嵌套分块均拒绝。超时、读写失败或请求 future 被丢弃后连接不可复用，不自动重发。依据固定版本官方 [rpc-frame.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/coding-agent/src/modes/rpc/rpc-frame.ts)；本地只读副本在 `reports/omp-upstream-rpc-frame-v18.4.4.ts`。

`ProbeClient` 核验可执行文件摘要，新建独立 home/agent/session/workspace，拒绝带项目配置的祖先目录，清空继承环境，仅保留 Windows 加载所需变量。使用明确无效的 DeepSeek 占位凭证、禁用工具/扩展/规则/技能/LSP。公共接口仅协商和 get_state，未开放 prompt。配置隔离依据 [dirs.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/utils/src/dirs.ts)。这不是操作系统级沙箱；实际 worker 的目录权限、工具审批和进程树限制仍须实现。

最终真实报告：`reports/omp-product-probe-20261009-144602.json`。已核验的本机 OMP 18.4.4 协商到 RPC2，negotiate_protocol/get_state 各一次，返回 deepseek/deepseek-flash、会话 ID、messageCount=0、工具数=0、无流式/压缩/排队/后台活动且 isSettled=true。模型 prompt 为 0，未继承真实凭证。关闭输入后进程退出码 0、forcedTermination=false。程序入口为 `crates/adapters/examples/omp_readonly_probe.rs`；报告文件和隔离目录必须是新的，避免覆盖旧证据或复用用户配置。

本批定位并修复三个实际兼容问题，失败报告保留：

- `144148/144314/144344` 在 ready 前退出。原生文件已解包，但 Rust canonicalize 的 `\\?\D:\...` 路径令 Bun 解析器报告模块不存在；正常执行审批下同样失败。改用 dunce 的等价常规 Windows 路径后恢复，不修改安装文件和系统权限。
- `144435` 已到握手，但启动的 available_commands_update 被严格通道拒绝。核对官方 rpc-mode.ts 后只允许该元数据通知及 setWidget 显示更新，其他宿主/工具/重试帧继续拒绝。
- `144518` 状态查询已通过，但输入流 shutdown 后仍须强制终止。现在取走并释放 ChildStdin 的 OS 管道句柄，最终 `144602` 自然退出。早期报告只证明查询及受控终止，不能称为自然退出通过。

测试覆盖跨分块 UTF-8、错误元数据/编码/顺序/长度、关联错误、宿主请求与重试拒绝、EOF/超时、取消等待后不可重发、诊断正文丢弃。大响应来自本地协议替身；真实 OMP 状态响应较小，尚未证明真实大模型输出的分块行为。实际启动诊断仅向产品返回有限错误类别；OMP 自身在独立目录生成的诊断日志不等同产品运行日志策略已完成。

该模块尚未连入产品配置 UI、主副指挥分配、持久派发账本或工具执行。目录能力继续保持未验证；无模型状态探测不证明 API 认证、编程、取消、恢复或 P3 完成。

直接下一步：把独占进程/分块读取复用到 worker 通道，连接既有 PromptTracker，定义 session ID 与 controller request ID 的持久身份契约；接本机凭证和审核过的工具，再贯通调度、独立验收、取消/故障恢复。不能直接将只读通道放开为任意 RPC 调用。

## 14:02 历史批次

2026-10-09，14:02 +08:00。原型已有无工具文本、取消、上下文恢复和控制器落盘的编程产物证据；产品工程刚开始迁移，不能把原型通过等同产品已接通。

## 已核对的基础

本机 `D:\on my pi\omp-zh\omp-windows-x64.exe` 为243411968字节，当前SHA-256仍为 `5644e5d7c7cddd851d79999dbc3b98363817178b33426b355a20fdcad41a3f54`，与此前18.4.4官方资产核验记录一致。本轮未运行真实模型、未读取用户凭证。旧报告 `workbench-probes/reports/omp-1791029020031.json` 记录RPC版本1及支持[1,2]。

官方18.4.4协议提供按请求id关联的prompt_result、独立会话静止状态及后台活动字段；agent_end不能单独代表整个请求的工作结束。依据为 [rpc-types.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/coding-agent/src/modes/rpc/rpc-types.ts) 与 [rpc-mode.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/coding-agent/src/modes/rpc/rpc-mode.ts)。这是产品迁移应采用的关联依据，原型仅等待agent_end的路径不直接照搬。

## 本批代码

`crates/adapters/src/omp_protocol.rs` 新增纯协议完成判定。分别接收prompt ACK和同请求prompt_result，允许终态先于ACK到达；其后必须读取新get_state，核对原session/provider/model，无流式、压缩、排队或待完成后台工作，才返回完成/取消/错误结果。请求id明确标为controller-rpc-request-id，不称为原生turn ID。

四组测试覆盖：ACK/agent_end/session_settled不能冒充完成；终态先到与后台工作；缺失/错误/重复关联及未调用agent；重试/模型fallback拒绝；取消/错误不误报成功；会话/模型变化和活动证据缺失。模块不启动进程、不处理工具授权、不保存思考或原始模型文本。尚未接入UI/调度，目录中的OMP能力继续保持未验证。

## 完整实施顺序（第一项只读部分已有上述证据，其余保持）

1. Rust独占进程传输：有界JSONL和RPC2分块解码、单一读取者、请求id与command严格关联、超时关闭、EOF/取消及子进程清理。先在独立产品测试目录做无模型握手、协商及状态查询，核对实际版本字段。
2. 启动配置与凭证：固定用户批准的可执行文件摘要、独立配置/会话目录、禁用自动重试/模型fallback；按产品本机配置注入指定provider凭证，禁止命令行密钥或原始错误落日志。新增OMP配置不会改变OpenAI主副指挥限制。
3. 执行身份：明确区分OMP提供的session ID与控制器request ID；扩展通用执行契约时保留旧Codex摘要兼容，不借用UI身份，也不把request ID当作可远端恢复的turn ID。完成回执必须绑定持久派发意图和原尝试。
4. 工具与产物：接入可审核的文件/命令工具权限和取消边界，再接独立验收。原型JSON单文件落盘仅作回归，不替代首期原生工具目标；OMP声明工具名不等于获得任意系统权限。
5. 恢复、预算和调度：唯一尝试先持久化再发送；丢ACK或终态缺失保持未知；恢复只能用原会话可验证历史，证据不足不重发。按实际模型、版本、工具调用、费用与取消结果记录能力。
6. 产品选择与真实验收：配置界面、只读详情、主副指挥分配、OMP执行与独立验证贯通；最后在用户本机凭证下验收真实工具编程、取消和断线恢复。首次完成这些前不把OMP标为已接入。

OMP以外，OpenAI/GPT、Claude、DeepSeek、Qwen、Kimi、xAI Grok API/CLI，Qoder/WorkBuddy已有桌面会话，以及并行隔离、效率与安装升级范围全部保留。完整P3/P6未通过。
