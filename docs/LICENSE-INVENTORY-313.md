# 313 分发许可来源：2026-10-24 增量核对，实际入包仍待

310 的 0.1.21 安装包目前只有 SQLite 专项许可。以下补充材料已保存，尚未成为该冻结包的载荷，不能据此认定完整许可或正式验收完成。原报告、失败回执、生产源码和 310 冻结包均保留。

## Rust 依赖与缺失原文

Windows x64、桌面 custom-protocol 的普通/构建图有 272 项：4 个 workspace、1 个 vendor 补丁、267 个 registry 包。267 份缓存归档与 Cargo.lock 摘要一致；元数据及 464 个候选许可/版权文件已核对。`runtime-graph.json` 的 226 项普通依赖闭包排除 proc-macro，但 metadata 可合并 host/build feature，属于保守范围，不能称为精准最终链接对象清单。

原六个缺少直接许可文件的 crate 现在已有来源材料：

- ts-rs 11.1.0：复用 vendor 来源记录中同一提交的上游 MIT 原文。
- webview2-com、webview2-com-macros、webview2-com-sys：已取得固定提交的 Rust wrapper MIT 原文。
- alloc-stdlib 0.3.0：固定提交 `0a81fd6928ea3b33c8cd484aa4575d50ffb98012` 的 LICENSE 现已 HTTP 200 取得，摘要 `c0c56f26d9c051cac4d200c34c84e7ae9aaa853e01a982a1df08b09931e518ae`。此前 TLS 失败保留；不同提交的 alloc-no-stdlib 文本没有被冒用。
- selectors 0.38.0：固定提交的两个候选根文件均为 HTTP 404，GitHub tree 也确认根目录没有许可证文件。crate 元数据与 lib.rs 声明 MPL-2.0；完整标准正文取自已验证的 cssparser 0.37.0 LICENSE，记录明确注明此来源，未冒称 selectors 自带根文件。

`mpl-source-materials.json` 保存 cssparser、cssparser-macros、dtoa-short、option-ext、selectors 的五份原始锁定源码归档，66 个归档文件全部与本机源码逐字节一致。包含 build-only 的 cssparser-macros 是保守保留。本项全文件核对仅覆盖这五个包；其余 registry 包原检查范围仍是归档摘要、元数据和选定许可文件。

## 前端真实产物来源

29 个已安装锁定 NPM 包的缓存 tarball SHA512，以及本机 manifest/选定许可原文，都与 package-lock 相符，见 `npm-cache-verification.json`。

`frontend-provenance.json` 的内存构建没有写 dist，生成的 5 个资产与已有 dist 逐字节相同，并记录 91 个模块。`embedded-assets.json` 又在冻结 GUI 内确认了全部 4 个 JS/CSS 资产的实际压缩字节，清单和 GUI 摘要相符；HTML 经 Tauri 转换，未声称原 dist HTML 直接内嵌。

`frontend-source-verification.json` 将其中 12 个有源文件的 NPM 模块与锁定归档核对，覆盖 @tauri-apps/api、react、react-dom、scheduler。另核对 Vite、Rolldown 及其 Windows 原生生成器的选定输入。两个虚拟模块通过已有构建模块表和相应版本生成器归因，不声称从源码逐字节提取了虚拟模块。

实际包含的 `@tauri-apps/api/external/tslib/tslib.es6.js` 带 Microsoft 独立许可头，已从核验后的原文件保留全文；不猜测独立 tslib 版本。Vite 和 Rolldown 的根许可、附带第三方通知也已保存。

## Rust 标准库与原生材料

- 实际 Rust 1.99.0 工具链的 `COPYRIGHT-library.html` 已原样保存，共 1,499,465 字节；来源与摘要见 `rust-library-notice.json`。它覆盖工具链库版权集合，不表示集合内每项均被本产品链接。
- WebView2 SDK 1.0.3800.47 官方 NuGet 已取得。x64 Loader 的三个文件与锁定 webview2-com-sys 归档、本机文件一致，静态库也与 release build 输入相同。SDK 的 Microsoft BSD 式 LICENSE、NOTICE、nuspec 已保留，不能用 Rust wrapper 的 MIT 许可替代。
- NSIS v3.11 固定提交 `7359413009afd4f0fff472d841fc2f2cc0e0a5f8` 源码归档已取得，SHA256 `b176182d7d37f564fb97f0b2b1f84f4651702a17d7b3b1ce6a0dc50652001365`，1,668 个源文件。归档只读检查，未整体解压或执行。源码 COPYING 与本机 COPYING 原始字节不同，但 CRLF/LF 归一后相同，分别保留原始摘要。原安装器脚本实际选择 LZMA，其 CPL-1.0 原文和相应源码包含在上述材料；版本一致不等于可复现构建证明。

## 后续执行

1. 补 NSIS 实际插件及其他嵌套原生组件的独立来源与通知，尤其当前安装器调用的 `nsis_tauri_utils.dll`；不能用 NSIS 总许可自动覆盖。
2. 汇总分发目录、组件范围与源码取得说明，保留 OR/AND 许可语义，不更改产品自身许可证。处理标准库版权页指向的具体义务及原生嵌套通知；完整 SBOM 尚未声明完成。
3. 再修改最小打包配置，生成新候选并检查实际许可/源码载荷。不得补写 310 冻结包或修改其 manifest。

证据位于 `reports/license-inventory-313/`；本日修改前文档副本为 `docs-before-20261024/`。本轮续接没有新增网络请求、模型调用或 Key 读取，没有插件操作，也没有重跑成功的 310/312 验证。原下载 session 68176 已消费完成，后续不要重新下载。完整许可实际入包及正式验收仍未完成，`releaseAccepted=false`。
