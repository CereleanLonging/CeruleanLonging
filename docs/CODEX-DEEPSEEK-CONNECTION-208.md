# 208 当前Codex与DeepSeek副指挥：一次接通验证

依据用户2026-10-19新决定：当前Codex聊天为主指挥，DeepSeek为副指挥。本次独立授权的Key只允许一次接通验证；不复用196的旧授权，不记录Key。

## 本次实际范围

1. 当前Codex通过本地工具提交真实结构化方案，绑定本聊天会话标识、操作ID、项目ID、用途和状态摘要。
2. 复用既有ApiClient向官方DeepSeek端点发出最多1次POST，模型deepseek-flash，90秒请求时限、最多2048输出token。请求用途仅为Advise，评议实际接线方案；无工具、无文件权限、无模型枚举、无自动重试。
3. 仅发送当前接线方案、公开配置和技术缺口摘要，不发送Key、用户原聊天、代码、文件内容或第三方应用聊天。认证头仅发送到供应商官方API。
4. 输出须符合现有DecisionEnvelope契约；记录供应商实际model、response ID、usage及请求摘要，拒绝越权动作、旧状态、工具调用与额外批准字段。
5. 请求前以create_new保存单次意图；未知或失败保留证据，不重发。Key通过stdin进入SecretInput，只在内存使用，客户端结束后归零其拥有的密钥字符串；不建立凭证文件、环境变量或命令行参数。不能声称清除网络库所有内存副本、聊天历史或撤销供应商Key。

## 证据与产品边界

新模块management_ingress和deputy_management用于复用契约与现有传输。management_connect是隔离接通执行器；不接管RuntimeHost账本，不派发子模型，不替用户批准，也不改变默认产品配置。

Codex会话标识来自本地宿主元数据，用于关联，不能充当OpenAI远程身份签名；不伪造API回执或零token用量。当前真实主指挥产生方案，外部工作台尚不能据此自行唤醒这个聊天。

后续仍需：将外部chief的请求/等待/回填接入同一RuntimeHost，增加持久来源字段、读取器兼容、用户批准与状态新鲜性复核；DeepSeek独立凭证路由和供应商对应预算/恢复证据；运行器等待外部chief、不因没有OpenAI Key误拒绝；最后在具体授权范围内验证真实三级闭环。旧OpenAI路径及已有货币预算不得静默降级。

官方依据：[Codex作为平台](https://developers.openai.com/blog/codex-as-a-platform)描述exec、SDK、app-server和MCP集成层；该文不能证明工作台可直接控制当前桌面聊天。本次使用当前会话既有本地工具，不新建Codex聊天、不安装插件。

完整正式验收尚未通过；207当前签名审计与206 WorkBuddy成功结论继续有效。
