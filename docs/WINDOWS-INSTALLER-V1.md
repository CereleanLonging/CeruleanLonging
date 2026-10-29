# Windows 安装包与版本清单

095 将原有桌面构建扩展为 NSIS 安装包。`desktop:build` 继续生成未捆绑的 release 程序；新增 `desktop:package` 完成核心、契约、前端、桌面与 NSIS 打包。Rust 依赖使用 `--locked`，Node 依赖沿仓库锁文件。首个产物为 Windows x64、0.1.0 内部验收版，尚未签名，不能视为完整 P6 发布。

```powershell
# 在项目根目录，使用已按 package-lock.json 安装的依赖及 Rust 工具链。
npm.cmd run desktop:package
```

常规输出为 `target/release/bundle/nsis/CeruleanLonging_0.1.0_x64-setup.exe`。安装模式限定当前用户，提供简体中文和英语；配置 `allowDowngrades: false`。WebView2 沿 Tauri 引导安装模式，缺少运行时时可能需要联网；本轮主机已具备运行环境，不代证干净机器或离线安装。

## 安装、卸载与活动任务

安装器随包安装桌面程序及 `cerulean-runtime.exe`。用户数据默认在 `%APPDATA%\local.ceruleanlonging.workbench\workspace`；外部编程项目继续保留在原路径。

实际生成的 Tauri 默认脚本在静默安装时会调用 Restart Manager 强制关闭占用主程序的进程。产品的 [安装钩子](../apps/desktop/src-tauri/installer/hooks.nsh)覆盖此宏，改为只检查、拒绝维护，不调用进程终止。机制依据本次锁定工具生成的 `installer.nsi/utils.nsh` 核对；上游宏不存在时直接编译失败，要求重新审阅。参考：[Tauri 安装模板](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)、[进程检查宏](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/utils.nsh)；运行验收以本地生成脚本为准。

维护前检查安装位置的桌面和核心二进制是否被占用，并独占打开默认核心目录中已有的写锁文件；不会创建、写入或截断该锁。遇到占用或无法确认，返回错误码 20，提示用户先通过工作台正常停止核心，再关闭窗口。不得为了安装或卸载强杀任务。核心位于另一数据目录时，安装位置的核心映像占用检查仍生效；并不因此宣称能发现所有外部构建和任意路径的数据状态。

卸载保留业务数据；本轮静默卸载及失败拒绝均验证了数据摘要不变。上游交互式卸载仍有“删除应用程序数据”的可选项；当前钩子遇到选中状态立即以 21 拒绝，提示取消勾选后卸载，不能进入递归数据删除。该分支使用实际 NSIS 钩子隔离验证；完整交互页面和独立数据清理流程仍待验收。

这个安装器还不是事务式升级工具：未实现自动备份、失败版本切换或未知桌面任务迁移。升级前使用 [冷备份](COLD-BACKUP-V1.md)保留原数据，不能把普通安装成功当作升级回滚已通过。

## 可审阅的交付文件

[最终内部包目录](../reports/p6-windows-package-final-20261012/manifest.json)提供 `setup.exe`、实际 NSIS 桌面映像、核心和 `manifest.json`。清单记录产品标识、版本、平台、构建输入摘要、每个文件的大小、SHA256 和实际 Authenticode 状态。当前状态均为 `NotSigned`，`releaseAccepted` 固定为 false，源代码/工具链摘要不能代替发布者签名。

Tauri 打包时将唯一 `TAURI_BUNDLE_TYPE_VAR_` 标记的三字节后缀从 `UNK` 改为 `NSS`，打包结束后恢复构建输入。首次安装核验因此正确发现“构建输入与安装映像不同”。现在针对本次未签名工具链，限定唯一标记和已知值，推导确切包内映像并核对整个文件 SHA256；不是忽略任意差异。已用实际安装文件核对。准备脚本拒绝修改带签名文件；后续签名应在最终映像准备后执行，再核对包内签名与更新行为。

```powershell
# 输出目录必须是 reports 下不存在的新目录，不覆盖已冻结产物。
.\scripts\freeze-windows-package.ps1 `
  -Installer .\target\release\bundle\nsis\CeruleanLonging_0.1.0_x64-setup.exe `
  -OutputDirectory D:\test\CeruleanLonging\reports\一个新的包目录
```

## 本轮检查与剩余门槛

实际隔离安装检查使用本机当前用户注册表、唯一测试安装目录和测试数据目录。先拒绝任何已有产品安装或设置，禁止覆盖用户安装；安装器自身按 `/S /NS` 运行，不创建桌面快捷方式、不启动产品界面。启动的核心仅连接测试数据，不调用模型；测试结束仅卸载测试产品、清理原本不存在且仍指向测试目录的产品注册项。

已验证：安装后桌面及核心完整映像一致；实际核心运行时重装和卸载都返回 20；原核心 PID 保持、安装文件不变；明确停止测试核心后卸载成功；卸载注册项和程序移除，默认真实数据目录与测试数据保持原摘要。失败日志、标记差异核查和最终产物摘要见 [095 验收索引](../reports/p6-installer-20261012.json)。

尚未通过：干净 Windows 目标与完整 WebView2 矩阵、安装后真实桌面编程、交互式数据删除、升级中断/磁盘满/失败自动恢复、版本回滚和数据转换、跨用户/机器迁移、证书签名及更新服务。完整 P3–P5 的真实供应商、两款桌面、编程与含失败效率等原要求也仍保留；本轮安装包不能替代这些验收。
