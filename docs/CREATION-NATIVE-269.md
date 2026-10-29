# 269 创建失败回读的原生桌面验证

2026-10-23T02:45:43.986466+08:00。在新隔离目录使用268源码修复及当前core/desktop构建，内嵌完整生产前端，通过真实Tauri命令和本机核心验证。未使用268测试钩子或模拟IPC，没有新生产源码改动。

## 实际结果

- actual native create rejects a stale-catalog directory conflict; UI re-reads partial entry and blocks open/recreate
- keep-original action only clears proposed identifier; explicit new native creation succeeds without opening another Host
- actual existing ready registration is discovered after native create rejection; no automatic open or replacement
- fresh native desktop reads the same partial and ready registrations; no file changes, replay or task dispatch

原生主窗口先读取空catalog，再由本次驱动在新目录创建占位目录和一个保留文件。UI提交同编号时原后端实际返回文件已存在（183），UI自动回读并显示缺少最终就绪登记；打开与重复创建被阻止。此受控占位不是产品创建中途写出的意图，不据此扩大故障覆盖。

点击保留/填写新编号，只重置输入，不发生创建；用户动作模拟器另点创建后exp-new269完整创建并保护成功。另一个实际Tauri调用先创建exp-ready269，旧列表中的UI再次提交该编号被真实后端拒绝，自动回读发现原完整登记且可显式打开，不自动打开或改写它。共2次成功创建及2次冲突拒绝，无原任务重发。

保存目录树/文件摘要后正常关闭原窗口，默认隔离Host继续为原PID；新桌面进程回读同3项（1未就绪、2就绪），catalog全部文件摘要保持，计划/workflow/API/run为空。随后正常关窗并service stop原隔离核心。部分目录、创建成功目录及重开证据均保留在live中，禁止重跑native.mjs覆盖或重新创建。

## 构建、观察与进程

完整前端及嵌入desktop/current core构建通过。首次Vite配置加载在沙箱spawn EPERM，frontend原日志保留；正常宿主frontend-host通过后仅续接尚未进行的构建/实机阶段。612源码文件在构建前后与归档一致；binaries固定。12捕获回执全部终态（初始frontend失败1，其余11成功），输出关闭摘要核对；2桌面/1核心原Windows句柄均确认退出，cleanup无问题，无强杀。截图live/native-creation-result.png已查看。268的65项证据保持。

WebView缓存与实时锁不计入不可变证据索引；原业务目录、前端、源码、程序、动作日志和原句柄回执均收录。只使用新测试工作台，未访问既有用户工作台、Qoder、WorkBuddy或用户Key。0模型/凭证，真实效率0/72。

冻结0.1.15/reader13发行包清单93b8ac9b76772d24ae0af1e43dfa899094d7d03ae3e2a3e78c744b729573e51f保持；本轮debug程序为开发reader15，不是新发行包，未安装、签名、创建密钥或变更信任。

## 未完成门槛

- 本轮是当前生产源码构建的debug嵌入界面与真实本机核心，不是已签名发行包或安装验收。新候选仍需核查当时实际证书、签名产物、版本和清单，及独立Windows安装。
- 原生故障是列表过期后的目录占用/原登记已就绪冲突；未在原生窗口中注入创建中途断电。268的两个内部文件系统故障仍按独立Rust证据继承，不冒充本轮原生动作。
- 便携证据归档仍待；保留入口只引导新编号，不生成持久处置决定、不删除原目录、不解除64项上限。
- 完整首批供应商/CLI、桌面故障矩阵、严格原桌面任务监督、真实金额门禁、真实效率72槽仍未整体签收。指定配置既有证据保持，不重跑238等原成功任务。

目标active、releaseAccepted=false。账户额度不可可靠读取，可靠95%账户信号到达安全收尾，不用上下文占用替代。238通过，不需前台/重发。
