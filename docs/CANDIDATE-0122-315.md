# 315：0.1.22 / reader16 内部候选已完成

2026-10-24 01:54:31 +0800。正式验收仍为 false；新候选未安装。清单 SHA256 `f1724261e037ae23f7b3e64ba77432d0afc2fba16c1e781ba897bc7bd0dcd8b7`。

314 的编码修复、五份许可/源码资源、实际载荷检查和同集合资源更新已进入新包。0.1.21→0.1.22 的资源集合变化要求完整安装器升级，不能用保留旧卸载器的二进制更新添加新文件。

本轮备份兼容检查49 passed/3 ignored，桌面维护3/3，严格 workspace/all-targets Clippy、fmt、release核心、前端、Tauri/NSIS及冻结/验包/实际载荷/原生依赖均通过。完整1253 passed/286 ignored和11项reader门禁引用310原证据；本轮没有重跑全量。构建时1110份冻结源码无漂移，五资源摘要和源码ZIP全部成员、GUI内嵌三脚本字节通过。

原session19327在桌面编译阶段失去句柄；原PID7912及编译进程均已不存在，停止原因未知、退出码未知。原release核心和前端完成日志保留，三份核心摘要及build-info确认一致。随后仅续接未完成Tauri/NSIS步骤（其beforeBuildCommand再次执行前端构建），没有重跑已通过Rust检查或原模型任务；新session78025完成且日志关闭。原package.started/stdout/stderr及package-interruption.json保留，没有伪造原package.receipt。详细阶段证据见 `reports/candidate-0122-315/candidate-ready.json`。

签名核查重新观察24个包，清单/字节/签名者无冲突。新包三份exe实际状态为 NotSigned；公开代码签名证书记录1条。没有使用测试证书、创建密钥或修改信任；正式发布身份与吊销仍未验收。旧310包保持。

278组件材料包括保守构建依赖，不能称为精确运行时SBOM。nsis-tauri-utils上游的精确依赖锁/构建工具链仍缺，完整分发许可审查未结束。独立Windows安装/完整升级/回退/卸载/WebView2、剩余供应商/CLI、严格桌面监督、逐笔费用/真实效率与长期历史开销等门槛保留。官方OpenAI Key实测继续按用户要求延期。

316随后只修改独立安装验收脚本和测试；不改变本候选二进制/清单或315冻结源码。当前活动源码与冻结源码的这两项差异见316结果，不能据此修改315原通过记录。
