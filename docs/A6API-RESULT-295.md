# 295：0.1.19 中转编程差异实测通过

2026-10-23。用户再次批准 A6API 中转调用和直接实测。本次使用 293 冻结的 0.1.19/reader16 发行核心（SHA-256 `aa5cbf4fd98dace69ef6cd48ef45368f8a1fcc7224c279b6034721530d3f8af0`），在全新 `relay295` 隔离项目完成 `product.mjs` 单任务。未重发 279、284、286 的历史请求。

主指挥 Plan、副指挥 Assign、主指挥 Accept 各一次，执行者 3 轮，共 **6 次真实 A6API / `gpt-6.1-sol` 请求**，低于冻结的 7 次上限。模型只读一次、带原摘要前置条件写一次 `product.mjs`；独立验证器 28/28 通过。工作流任务为 accepted/verified，项目运行为 completed。正常退出并两次重开后，数据逐字节一致，重复请求 0。

本次从当前聊天中用户先前提供的 A6API 回答取 Key，只经父进程私有 stdin → 执行器 stdin → 产品临时 DPAPI 槽传递；没有明文文件、命令行或环境变量中的 Key。临时槽已由产品命令删除，295 目录 413 份本地文件明文扫描 0 命中，桥接回执只记录状态。原用户聊天消息及服务端记录不属于本机清理范围。使用量合计输入 43,695、输出 923 tokens；缓存读取字段 4/6 缺失，缓存写入字段 6/6 缺失，实际金额仍为 `null`，需要提供方逐笔账单才能核销。账户额度没有可靠实时读数，不能声称 95% 阈值正在自动监控。

本次证明冻结 0.1.19 核心在所测 A6API 单任务范围内可真实规划、分配、编程、独立验收与恢复。它不证明 DeepSeek 规划＋GPT 执行的异构真实组合、其他首批供应商/CLI、金额预算、72 个效率样本、桌面严格原任务监督、独立 Windows 安装或正式签名。`releaseAccepted=false`。

证据：[295 审计索引](../reports/a6api-product-delta-295/latest.json)、[冻结计划](../reports/a6api-product-delta-295/live/authorized-plan.json)、[终态](../reports/a6api-product-delta-295/live/outcome.json)、[独立验收摘要](../reports/a6api-product-delta-295/live/verification-summary.json)、[两次重开](../reports/a6api-product-delta-295/live/reopened.json)、[凭证清理](../reports/a6api-product-delta-295/live/credential-cleanup.json)、[明文扫描](../reports/a6api-product-delta-295/credential-trace-audit.json)、[桥接记录](../reports/a6api-product-delta-295/live/credential-bridge-result.json)。295 执行入口不可重跑；原 286/293 冻结材料保持。
