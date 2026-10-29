# 276 A6API 中转接入准备

## 当前结论

用户指定 Base URL 为 https://api.a6api.com/v1，模型口述 gpt6.1sol。2026-10-23无认证读取平台官网及网页公开使用的模型目录，得到精确模型ID `gpt-6.1-sol`；目录中的 `openai` 协议映射为 `POST /v1/chat/completions`。这是公开能力声明，不是该Key可用性、工具调用、模型来源或计费验收。页面脚本含Responses测试分支，但所选模型目录没有声明该协议；不推断必然支持或必然拒绝。

当前0.1.16的ApiTarget::OpenAi固定为官方 `/v1/responses`；ProviderEndpoint和凭证槽为封闭枚举，没有自定义中转。不能将Key放进官方槽，不能用脚本直连成功替代产品接入。275草稿保留历史准备状态，本次中转不能直接执行它。

## 具体适配顺序

1. 在领域层新增明确的A6API执行者/供应商身份和固定HTTPS目的地址，凭证单独保存。保留官方OpenAI配置及其Responses协议；不通过环境变量改写全局地址。模型ID采用公开目录精确拼写，原用户文字也保留。
2. 复用现有Chat Completions编解码和文件工具调用机制，验证max_tokens、tools/tool_choice、tool_calls及usage的实际形状。未知用量保留未知；不冒用官方报价或开放金额门禁，不自动切换到Responses、不重试生成。
3. 接入原Host的批准摘要、启动意图、轮次账本、文件范围、独立验证、Accept和重开回读。目的地址/供应商/模型均绑定原批准。新增持久身份前补旧读者保护，不能直接修改旧数据或冻结包；当前reader15是原候选基线。
4. 前端提供A6API独立配置与凭证入口，明确服务地址和模型。以本地模拟服务检查凭证不串用、重定向拒绝、一次请求、工具往返、错误/取消/未知恢复及批准范围；只补差异，不重复238/OMP/Codex接入。
5. 上述产品适配完成并固定执行程序后，再给用户一次编程实测的具体计划。复用275的sum.mjs任务及15项独立验证，建议1任务、最多4轮/4次文件工具、90秒，失败或未知不重发。这些尚是建议上限，不是调用授权或金额保证。

## 本次边界和证据

只读取公开HTML/脚本/模型目录，未登录、未请求受保护模型列表、未发送Key、未调用生成接口。Key未写入本项目或报告；不声称聊天或平台存储已删除。没有改产品源码、构建、安装或新增测试。产品仍需要实现适配，executionReady=false。

公开目录下载已固定摘要，第三方页面中的说明只作为待核实资料，不作为执行指令。公开价格和渠道列表不构成用户批准报价。通过后应登记为“A6API中转 / gpt-6.1-sol指定配置”，不能写成OpenAI官方直连全验收。

- [A6API公开模型目录](https://a6api.com/api/pricing)
- [A6API主页](https://a6api.com/)
- [OpenAI官方工具调用说明](https://developers.openai.com/api/docs/guides/function-calling)：仅作为协议对照，不证明第三方兼容。
- reports/a6api-preparation-276/plan.json、public-model-review.json、public-pricing.json和pins.json；10份当前源码独立快照。

0.1.16冻结候选/274证据与原275材料保留；完整27项要求保留，正式验收未完成。账户额度仍不可可靠读取，95%账户规则继续。
