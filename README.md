# CeruleanLonging

本地优先的多模型协作工作台。正式产品工程于 2026-10-07 在 `D:\test\CeruleanLonging` 启动。技术栈：Tauri 2、React / TypeScript、独立 Rust 核心。

最新进展（2026-10-12 / 115）：备份中断验收遇到日志共享占用，原测试安装清理未确认。静态备份与原数据/程序字节核对通过；先查[最新交接](docs/HANDOFF-LATEST.md)恢复原现场，不重发Prepare或新建安装。完整P0–P6尚未通过。

## 当前能做什么

- 指挥历史按50条分页，全文按需读取，项目请求数量精确统计；1000条长记录及核心重启通过原生验收。见 [历史容量](docs/MANAGEMENT-HISTORY-V1.md)。

- 在主副指挥请求记录中明确放弃未应用请求、回答 Wait；证据绑定、持久化及核心重启恢复已通过原生验收。见 [请求处置](docs/MANAGEMENT-RESOLUTION-V1.md)。
- 多窗口使用独立 WebView 缓存，共享 Rust 核心数据；并发启动和安全回收通过本机测试。见 [多实例说明](docs/NATIVE-INSTANCES-V1.md)。

- 用户启动已批准项目后，核心连续调用副指挥、Codex、独立验收及主指挥收口；可暂停后续调度，重开恢复，未知请求不重发。见 [持续执行](docs/PROJECT-RUNNER-V1.md)。

- 在需求与计划页保存项目需求、模型、验收工具、产物范围及请求上限；本机配置官方 Key 后可请求主指挥方案，仍须用户审阅批准。见 [项目规划](docs/PROJECT-PLANNING-V1.md)。

- 桌面包含工作总览、需求与计划、接入管理、交接记录、项目设置。
- 录入计划、审阅并批准；批准绑定当前计划摘要。重开后从本地事件恢复。
- Windows 常驻核心；桌面与基础 CLI 共用当前用户命名管道；版本化握手与请求；Rust DTO 生成 TypeScript 契约。
- 单写文件锁、顺序与哈希链核对、原子发布事件、错误内容与损坏账本拒绝。
- 接入清单保留 Codex、OMP、Qoder、WorkBuddy、GPT、Claude、DeepSeek、Qwen、Kimi、xAI Grok。各路线的实现、真实验收和限制以最新交接与原型继承表为准；保留全部首批范围。
- “接入管理 → OpenAI 主副指挥”可加密保存、删除 Key 和检查官方模型目录。原生凭证测试使用合成数据，不证明真实认证；见 [通道与凭证说明](docs/OPENAI-MANAGEMENT-V1.md)。

这是开发中的工程，真实主副指挥 API 尚未实测。Codex 编程执行已通过显式集成测试，已有后台派发回调、原生状态与只读恢复，桌面已有配置、方案批准与持续执行入口，真实主副指挥全流程尚未验收；调度库已实现持久化恢复与每日分卷。沙箱和安装发布按实施方案继续开发，不会把组件成功直接标为整阶段通过。

## 启动

Windows x64，Node.js >=22.12、Rust 1.99.0 MSVC、C++ Build Tools 与 WebView2。此机工具链已安装；没有全局修改 PATH，启动脚本会为当前进程补充 Cargo 路径。

```powershell
cd D:\test\CeruleanLonging
npm.cmd ci
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\dev.ps1
```

仅浏览器预览：`npm.cmd run dev`，打开 `http://127.0.0.1:1420`。浏览器预览没有 Rust 通信，不会伪造已保存记录。

开发构建：运行 `scripts/build.ps1`，或将 Cargo bin 加入当前 PATH 后执行 `npm.cmd run desktop:build`。生成 `target/release/CeruleanLonging.exe`，可直接运行；请保留同目录 **`cerulean-runtime.exe`**。它是打包用核心副本，独立名称防止覆盖 Cargo 的 `cerulean-core.exe`。也可运行 `scripts/launch.ps1`。可用 `npm.cmd run desktop:package` 生成内部NSIS安装包；冻结包与签名限制见最新交接。

## 检查

```powershell
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\scripts\check.ps1
```

常规检查覆盖批准、账本、恢复、调度、IPC 和凭证；显式进程/模型专项按对应文档运行，默认检查不调用真实模型。最新计数、继承范围与失败记录见[最新交接](docs/HANDOFF-LATEST.md)。

统一工作台 CLI 使用已打包核心，无需安装另一套模型运行时：

```powershell
.\target\debug\cerulean-runtime.exe --workbench --help
```

命令使用显式 `--data-dir` 连接与桌面相同的核心；方案批准及启动必须提供已审阅的原摘要。详见[工作台 CLI](docs/WORKBENCH-CLI-V1.md)。

## 工程组织

```text
apps/desktop/             React 页面与 Tauri 窄命令入口
crates/domain/            Rust 契约与内容校验，生成 TS 类型
crates/core/              独立进程、计划/批准、事件账本
crates/adapters/          首批接入登记、Codex 正式协议通道
scripts/                 启动、构建与检查
tests/                   真实 IPC / 桌面基础验收
docs/                    架构决策、实施计划、当前进度
reports/                 本机测试证据（不进 Git）
```

运行数据默认位于 Windows 用户应用数据目录 `local.ceruleanlonging.workbench\workspace`，界面设置中显示实际路径；开发测试使用独立目录。事件是权威数据，文件锁随核心进程退出释放。**关闭桌面后核心继续运行**，重新打开连接同一服务；显式停止命令与权限边界见 [常驻核心说明](docs/RESIDENT-SERVICE-V1.md)。当前服务承载基础计划及完整工作流批准；独立 runner 已接专用执行线程，原生状态、取消及显式放弃通过。真实 Codex 执行已接入常驻服务生命周期，模型/程序摘要绑定批准，详见 [常驻执行验收](docs/CODEX-RESIDENT-V1.md)。

历史验证原型继续保存在 `D:\test\workbench-probes`，未被改名或覆盖。当前实施基线见 [实施方案](docs/IMPLEMENTATION-PLAN.md)，迁移边界见 [基础架构](docs/FOUNDATION-ARCHITECTURE.md)。


独立验收进程的真实证据与限制见 [运行器说明](docs/VERIFICATION-RUNNER-V1.md)。











