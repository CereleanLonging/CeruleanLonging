# 原生窗口并行运行与 WebView 缓存隔离 v1

日期：2026-10-08。此项解决桌面进程同时运行时的 WebView2 初始化冲突，不代表 P6 总体验收完成。

## 原因与范围

保留的失败证据是 `reports/desktop-smoke-WNq5RT/failure.json`：桌面启动退出 101，WebView2 报 `0x8007139F`。并行 debug/release 测试使用不同 `remote-debugging-port`，此前却共用默认 WebView 数据目录。

Tauri 官方说明：具有不同 browser arguments 的 WebView 实例必须使用不同 data directories。此约束与这里的触发条件吻合，修复因此放在桌面环境创建处；不能仅通过串行测试隐藏问题。[Tauri WebviewWindowBuilder](https://docs.rs/tauri/2.12.1/tauri/webview/struct.WebviewWindowBuilder.html#method.additional_browser_args)

## 实现

- `tauri.conf.json` 中 main 窗口设置 `create: false`，由 `setup` 使用原配置创建；尺寸、标题、安全权限保持同一来源。
- 每个桌面进程在 `%LOCALAPPDATA%/local.ceruleanlonging.workbench/webview-instances-v1/` 下建立新的 `instance-<PID>-<纳秒时间>-<序号>` 子目录。WebViewWindowBuilder 显式指定这个绝对数据目录。
- `webview_profile::Profile` 保存在 Tauri 托管状态中，持有 `.cerulean-owner` 文件的操作系统锁直至该进程退出。创建通过 `create_dir`/`create_new` 检查冲突，不复用旧目录。
- 业务目录仍为 `%APPDATA%/local.ceruleanlonging.workbench/workspace`。计划、审批、执行记录和加密凭证仍由原常驻 Rust 核心管理；关闭窗口不终止该核心。
- debug 版本保留隔离业务数据变量 `CERULEAN_TEST_DATA_DIR`，另提供 `CERULEAN_TEST_WEBVIEW_ROOT` 供缓存拒绝清理测试使用。release 不使用这两个测试变量。
- WebView 的 cookie、localStorage 等属于进程缓存，不承担持久业务状态。以后增加需要跨窗口保存的界面偏好，应通过核心或独立的持久设置实现。

## 安全回收边界

每次启动最多成功回收两个已退出进程的缓存，最多检查根目录前 128 项。预检查共用 250 ms 预算，单个候选最多 10,000 个目录/文件、32 层深度；超限保留。已经开始的受限文件删除完成后返回，因此该预算不是整个系统调用耗时的硬实时上限。

回收要求全部满足：

1. 根目录、候选目录、祖先及被枚举项均为普通文件/目录；拒绝符号链接、junction 和其他 reparse points。
2. 目录名符合固定格式，规范化后的父目录等于此缓存根；owner 文件含精确的版本和本目录名。
3. 能非阻塞取得 owner 文件锁；活动桌面实例的目录立即跳过。
4. 删除前完成整树检查，并对每个缓存文件持有禁止其他读写、允许删除的 Windows 句柄。如果桌面已退出但 WebView 子进程仍占用文件，会在删除任何缓存内容前放弃该候选。
5. 先删除已验证缓存项，再最后删除 owner 文件及空目录。中途失败保留可识别标记；不会改名移动业务目录，也不会清理默认旧版 WebView 目录或其他程序目录。

锁文件采用标准库 `File::lock` / `try_lock`；锁随文件句柄关闭而释放。递归删除仅用于已验证候选中的普通目录，Windows 标准库另提供符号链接替换竞态防护。[Rust File 锁](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)、[Rust remove_dir_all](https://doc.rust-lang.org/std/fs/fn.remove_dir_all.html)

无法确认安全的缓存会保留，极端情况下需要后续明确的维护工具处理；不会为了消除磁盘残留扩大删除范围。进程崩溃且没写完 owner 的目录同样保留。

## 验证

模块测试已由单独的 `rustc --test` 执行，日志为 `reports/webview-profile-module-tests-20261008.log`，3 项通过：

| 用例 | 证明的范围 |
| --- | --- |
| 多个存活 Profile 与退休 Profile | 同时存活目录不同；活跃锁不删除；已退出且有正确标记的缓存可以回收 |
| 外来目录及标记不匹配 | 不删除未识别目录、其他应用标记及越界名称 |
| WebView 遗留文件句柄 | 桌面 owner 锁释放后，仍被其他读写句柄使用的缓存整体保留；句柄关闭后可回收 |

工作区构建后可复核：

```powershell
cargo test -p cerulean-desktop webview_profile --locked
node tests/native-instances.mjs
```

新增原生测试 `tests/native-instances.mjs` 要求已经构建 debug 和 release，不负责重新编译。它以不同 CDP 端口同时启动两个 debug 和一个品牌 release，所有实例使用相同的默认缓存根，检查：

- 三个 WebView 使用不同缓存目录，均成功打开；原失败 `0x8007139F` 未复现。
- 两个 debug 窗口共享测试业务核心，一边创建计划，另一边审批，双方原生 IPC 读取相同结果。
- 一个窗口关闭后另一个仍正常；重开获得新缓存目录，计划 digest 及审批保留；所有窗口关闭后常驻核心仍可读取记录。
- 新窗口清理时保留仍活跃的兄弟及 release 缓存；release 业务数据仅作读取，测试不创建计划或调用模型。
- 额外隔离测试在已退休缓存内加入指向报告目录其他位置的 junction，确认候选及目标哨兵文件均保留；无法识别的目录也保留。

测试写入独立 `reports/native-instances-*/result.json` 或 `failure.json`，失败不覆盖旧证据。只关闭测试创建的窗口；只停止独立测试核心及本次新启动的 release 核心，保留事先存在的 release 核心。全部模型调用数为 0。

当前 UI 空闲时不自动轮询其他窗口修改，因此该测试明确刷新第二个 WebView 后进行人工审批界面操作；它证明共享核心与持久化，不声称跨窗口界面自动同步已实现。

原生集成结果须以本次实际生成的 `result.json` 为准；本文列出测试覆盖范围，不能将尚未执行的检查计为通过。此项不证明所有安装环境、系统 WebView2 版本、运行一整天的稳定性或产品其余阶段已验收。

## 本机实测结果（13:47 +08:00）

统一发布/调试构建后已运行原生并发测试，reports/native-instances-YF3yL4/result.json 全部通过。两个 debug 与一个 release 同时启动、不同端口/缓存目录、共享计划及批准、窗口关闭/重开、活跃缓存保留、junction 与异物目录拒绝回收均取得本机证据；发布业务数据只读。原故障未复现，旧失败报告仍保留。parallel-approved.png 已目视检查。最新 Cargo 模块测试 p2-native-profile-final-tests-20261008.log 3 项通过；最终 clippy 通过。模型调用 0，完整 P6 仍未通过。
