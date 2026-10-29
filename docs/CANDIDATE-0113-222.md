# 0.1.13内部候选的验收范围与后续顺序

2026-10-20T18:47:22.926608+08:00。218原授权已执行并消费：真实三级编程闭环通过、临时凭证已删除。本批没有复用Key、调用模型、发送桌面消息、安装应用或改变信任。

## 已完成与继承

- 版本从0.1.12同步至0.1.13：workspace、四个Rust锁记录、npm清单/锁、Tauri配置一致；reader保持13，备份写入者保留0.1.0至0.1.13。
- 冻结1205份源码，保留56份旧包文件与14份会被构建替换的产物；复用当前target缓存、禁用incremental，没有复制新的target树。未覆盖其他模型已有修改或旧候选。
- 当前普通Rust回归累计1096通过、226项既有专项/实机测试忽略；忽略不算通过。原workspace因进程丢失无最终回执，继承明确通过的440项核心及适配层结果，仅续接365项核心和未执行目标，未伪造原命令成功。严格Clippy、fmt、TypeScript/Vite及续接NSIS构建通过，原中断日志保留。
- 新包独立放在reports/p6-windows-package-0113-20261020，清单SHA256为f65d2065eb0d84f909204362fa3814ca0182b23a94d5eb04d79aae6733c0b67b。包清单、实际build-info、三个产物摘要、内嵌维护脚本和原生观察桥一致。
- 第一次freeze在脚本启动前被Windows PowerShell执行策略拒绝，没有生成归档；保留失败回执，改用既有PowerShell 7及仅当前进程的脚本参数后成功。系统执行策略、证书信任均未修改；包构建本身未重跑。
- 打包后的0.1.13核心无凭证回读218原项目与最终Accept；项目和全部原数据逐字节不变，自有核心正常停止，0重发、0强杀。继承196 OMP真实监督/收口、206 WorkBuddy原203收口、218所选真实三级闭环，不能据下方历史待办重复派发。
- 新隔离目录通过真实0.1.12备份→0.1.13 reader13提升→新版本冷备份与逐字节恢复，旧reader12实际拒读且原文件不变，所有自有核心正常停止。没有安装GUI或执行程序目录替换，不能代证这两项。
- 本轮重新审计15份包清单、95次产物观察，公开证书元数据10条；未发现清单/版本/摘要/已登记签名者冲突。离线结果：{"0x800B0100": 95}。新包实际状态：setup.exe: NotSigned, CeruleanLonging.exe: NotSigned, cerulean-runtime.exe: NotSigned。没有新建密钥、签名、改信任或外发；不把测试证书和离线检查当正式发布验收。

## 下一步与最小续接

1. 先读docs/HANDOFF-LATEST.md、docs/CANDIDATE-0113-222.md及reports/candidate-0113-222/candidate-ready.json；新候选已完成构建/只读验包，不要重建或重跑218。后续使用新包前重新核对清单摘要与当前产物。
2. 继承本批实际隔离跨版本备份/reader13恢复及210证据，继续补安装后GUI、内嵌完整payload及程序替换/回滚。当前账户仍登记D:\on my pi\omp-zh\AppData\Local\CeruleanLonging的0.1.7，已只读保存登记，未启动安装器、未覆盖旧安装；不能把当前账户当干净环境。隔离恢复不能随意改写原路径绑定。
3. 桌面严格原请求监督仍待受支持传输。197/207/220已排除所查普通发消息、queue及评测chat/inject入口的等价性，不再重复同一调查，也不能把排队发送当纠偏消费确认。
4. 首批真实供应商矩阵、生产预算差异与可靠性/效率仍按原验收标准补齐。新增真实调用/桌面发送须有对应范围授权，218一次性授权已消费，不复用历史Key。
5. 独立干净Windows环境和确定发布者的正式签名仍是发布前条件。用户此前尚无环境/证书；本轮未获取新条件，也未授权新密钥、信任变更或外发。具备条件后按当时实际状态核查，不沿用旧未签名结论。

证据：reports/candidate-0113-222/candidate-ready.json、test-coverage.json、source-snapshot.json、embedded-components.json、signature-summary.json、signature-audit.json、recovery218/result.json、maintenance/result.json、resume-plan.json、installation-preflight-readonly.json。原构建日志及旧报告保留。

goalStatus=active，releaseAccepted=false。用户已明确恢复任务，此前02:48:44账户95%触发记录保留；没有恢复后的可靠实时读数，不宣称阈值监控有效。再次收到可靠已用95%读数时安全收尾、停止新增阶段并等待恢复。不得以上下文或token计数代替，不关机。
