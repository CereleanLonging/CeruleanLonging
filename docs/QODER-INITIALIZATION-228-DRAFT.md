# 228 原草稿续接

2026-10-20，用户批准焦点修复后继续原两条消息。第一次输入已发生：原计划第1条的86个字符全部输入，未发送。原助手脚本固定等200ms后回读过早，退出；后续只读回查确认完整原草稿逐字相同，发送按钮已可用，发送意图不存在。第二条尚未开始。

这是初始化工具的异步回读等待缺陷，不是Qwen模型失败；本轮未产生供应商调用。原输入意图与日志保留。`v2-first-composed.json` 根据只读回查补记原输入完成，不重复输入，不清空草稿，不创建第二份任务。

当前现场：

- Qoder PID11116，窗口329806，0.4.3.0。
- 隔离工作区“工作台 Qwen 验收 228”，项目目录 `D:\test\CeruleanLonging\reports\qoder-product-228\project`。
- 自定义Qwen-3.8-Flash，当前设置页已核对阿里云中国区Token Plan。
- 输入框保留原计划第1条，标记 `CL_QWEN_INIT_228_A_764390ef`，草稿86字符。
- 0发送意图、0调用，第二条尚未输入；禁止再次执行Compose。

准备好的 `reports/qoder-product-driver-228/initialization/resume-first-draft.ps1` 只接受Send阶段，重新核对模型/工作区/空项目/原草稿/唯一发送按钮后，保存独占意图并仅Invoke一次。任何已存在发送意图都拒绝；未知只回读，不重发。第二条继续沿原两条范围，必须在第一条成功、新UUID及cwd核对后进行。

等待用户允许续接原草稿，原因是前次约定失败即停；不把这次局部脚本失败当作重新验证整个接入的理由。续接不是重新输入，更不是重复发送。最终仍最多两条初始化消息，不创建文件、不调用工具、不运行命令；不包含后续sum编程任务。

证据：`reports/qoder-product-driver-228/initialization/v2-readback.json`、`v2-first-composed.json`、`v2-result.json`。原228产品受控演练、fmt与Clippy成果保持，无需重跑。正式验收仍未完成。
