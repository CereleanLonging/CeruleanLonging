# Windows 常驻核心与本机 IPC v1

2026-10-07。本轮将桌面基础命令从私有 stdio 子进程迁移到当前用户的本机命名管道。核心独立于窗口，桌面关闭不发送退出请求；重新打开连接同一服务。当前服务仍处理基础 Store 的 snapshot/createPlan/approvePlan，尚未承载完整 Workflow、模型执行和验收器。

## 实现与协议

- `crates/core/src/ipc.rs`：服务、客户端、OS 身份与 ACL、握手、分帧、连接数/时间限制。
- `cerulean-core --serve --data-dir <目录>` 启动服务。Tauri 使用同目录打包副本 cerulean-runtime.exe 按需启动，无可见终端。
- 核心持有原 Store 文件锁，另设 FIRST_PIPE_INSTANCE；重复启动拒绝，不接管已有进程。客户端仅在请求发送前重连/等待启动，发送后失败不自动重放。
- 管道名由当前用户 SID 和规范化数据目录摘要派生，拒绝远程客户端。每个连接仅接受一个命令，最多同时处理 16 个连接。
- 帧为 4 字节小端长度加 UTF-8 JSON；先检查长度再分配。请求/握手上限 64 KiB，响应 4 MiB，单连接操作 15 秒。孤立慢客户端不阻止其他正常连接。
- 服务端先发 product/protocolVersion/serverPid/ownerSid。两端从实际管道查询对方 PID、再核对进程令牌 SID；客户端检查握手与 OS 观测一致。协议不符拒绝命令。
- 用户通道不接受 principal，不能发送 WorkflowCommand。用户命令由服务端处理，现已增加 registerProject 和 approveRevision；关闭服务是独立 shutdown 操作。

仅在 Windows 支持本模块。原 --stdio 保留为兼容测试入口，不再由桌面生命周期持有。没有启动项、系统服务注册或自动登录驻留；是用户会话中的普通本地进程。

## 权限边界

创建管道时使用显式、受保护 DACL，唯一允许项是当前用户 SID。单元测试读取实际管道的 ACL 并核对只有这一个允许项，避免仅检查字符串配置。两端核验 PID/SID进一步拒绝其他用户进程。依据：[微软命名管道权限文档](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)、[客户端进程 ID 接口](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid)。默认 DACL 可能包含其他读者，因此本实现不使用默认值。

这不隔离同一用户下的任意代码：同账户进程仍可连接、占用或终止服务。伪造 JSON 的 principal 被拒绝，只证明不能通过字段升级角色，不能证明本机 Agent 已被 OS 沙箱隔离。未来 worker 工具权限/沙箱、凭证隔离和可信主管通道仍须单独落实。当前未进行其他 Windows 账号或不同登录会话的实机测试。

## 命令行与退出

CLI 共用服务的基础命令，例如从工程根目录运行：

```powershell
# 服务已由桌面启动时，读取同一份记录。
'{"id":"manual-snapshot","protocolVersion":1,"command":{"type":"snapshot"}}' |
  .\target\release\cerulean-runtime.exe --request --data-dir "$env:APPDATA\local.ceruleanlonging.workbench\workspace"

# 明确停止本用户此目录的核心。只停止核心，不关闭第三方应用。
.\target\release\cerulean-runtime.exe --stop --data-dir "$env:APPDATA\local.ceruleanlonging.workbench\workspace"
```

--request 不自动启动服务；桌面会在用户下一次请求之前按需启动。--endpoint 可查看本目录管道标识。CLI 目前是协议级入口，面向用户的 plan/approve/task 等易用命令以及全部模型调用仍在后续阶段。

服务收到 shutdown 后不接受新连接，等待已接受请求结束，最长再等待 15 秒。23:24 已接 Workflow：未完成任务存在时返回 unresolvedTasks 并保留服务；完整停止派发/取消/对账和托盘退出流程仍待实现。不能把本轮窗口生命周期验证当作长任务保活已经验收。

## 检查与限制

真实服务测试验证：多个客户端共享计划/批准；客户端断开后同一 PID 继续服务；第二服务拒绝；退出后重启恢复；CLI 读取相同记录；错误版本、过大帧、角色注入不写入；慢客户端不阻塞其他连接。Rust 额外验证两个客户端并发按需启动使用同一 PID、shutdown 后文件锁释放。

桌面测试分别检查发布窗口关闭后 CLI 仍能读取核心，以及隔离数据目录内创建/批准/关闭/重开。测试结束仅停止测试创建的核心；已有发布服务保留。数据未注入用户日常项目。

更新：完整契约持久化与服务绑定、用户 UI 批准、严格产物收口已通过，见 [严格宿主](STRICT-HOST-V1.md)。剩余：真实模型调度通道；runner 的异步服务集成与不可变快照；事件订阅游标；SQLite/大账本/备份；GUI 完全退出与托盘；异常下异步磁盘队列。当前 Store 的磁盘提交是同步的，连接数有界不代表任意慢磁盘操作均能被硬中止。P1 整体仍未放行。


