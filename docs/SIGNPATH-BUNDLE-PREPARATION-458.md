# 保留已核对程序字节的打包入口

本项续接447本地 SignPath 准备方案，不公开项目、不上传、不申请证书或修改本机信任。原0.1.27候选及旧冻结包不变。

新增 `scripts/bundle-reviewed-windows.py`，使用已安装的 Tauri 2.12.1 `bundle --no-sign --no-binary-patching`。输入是独立目录中的 `input-manifest.json`、两个程序及五件资源文件。调用者必须提供该清单的确切 SHA256、预先审阅的构建输入摘要和新容量回执。

GUI必须在签名前就具有唯一的 `TAURI_BUNDLE_TYPE_VAR_NSS` 标记。本入口拒绝UNK或重复标记，不修补已签文件。它在新建的 `reports/bundle-reviewed-*` 目录中复制输入，并核对Cargo metadata确实选择该目录的target；只请求打包，不执行 `desktop.mjs package` 或编译命令。打包前后锁定、核对输入文件，最后核对NSIS内嵌的全部七个文件。原334进程观察器按原字节提升为通用脚本，保持原句柄及双EOF收尾。

实测见 `reports/signpath-bundle-458/result.json` 与 `reports/bundle-reviewed-458/result.json`：7项边界检查通过，1次实际打包成功；Tauri日志明确跳过二进制改写，七件内嵌文件与原输入相同。原0121至0127冻结包保持。测试使用未签名0.1.27输入；未使用真实证书、未验证签名链、未安装程序。新输出清单标为packaging-byte-preservation-check，不是正式发行manifest。

后续真实流程仍须先完成NSS归一化，再获得内层GUI/runtime签名及可核查回执，更新输入清单为签后字节后使用本入口。对外层安装器另行签名、验证签名链/时间戳/吊销与内嵌载荷，最后生成正式发行清单。当前payload读取器仅接受未追加签名的NSIS外层结构；正式外层签名后的载荷审计仍须补齐并测试，不能直接把当前通过结论外推。原 `freeze-windows-package.ps1` 仍拒绝已签GUI的归一化；不可拿它改写签名文件。

公开许可、联系人、SignPath审核、GitHub托管构建路径适配、真实签名验证，以及各供应商和安装验收仍未完成。447原方案保留，本说明仅更新已实做的本地打包准备部分。
