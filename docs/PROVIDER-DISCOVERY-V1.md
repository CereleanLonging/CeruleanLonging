# 首期 API 凭证与模型目录

2026-10-10，第027项。此模块为 Claude、DeepSeek、Qwen、Kimi 和 xAI Grok 提供产品内的独立凭证槽及官方模型目录检查；OpenAI 沿用既有主副指挥接入。目录检查不授予任务、工具或管理角色权限，不能替代真实生成与编程验收。

## 产品路径

在“接入管理”中选择对应供应商，将 Key 输入本机密码框并加密保存，然后显式点击“读取模型列表”。保存、替换、删除均只影响相应供应商；输入在提交后清空。DPAPI 绑定当前 Windows 用户、工作台目录及供应商，旧 OpenAI/DeepSeek 文件名与熵规则保持。

检查只发一条 GET，不带提示词，不自动重试、跟随重定向或切换地域。结果通过核心异步返回，UI 持续轮询。检查中不能替换同供应商凭证，核心不能正常退出；最长 HTTP 请求时间45秒、连接15秒。OMP 持有执行线程期间拒绝删除/替换 DeepSeek Key。原先凭证入口仍写着“OMP未接入”的注释，且没有这项保护，本阶段补齐。

状态有“未配置”“已保存未检查”“检查中”“列表已读取”“检查失败”。读取成功仅指该端点本次返回合格目录，未证明余额、特定模型权限、生成、流式、工具、取消或恢复可用。模型列表、结果和地域选择为本次检查信息，不作为持久模型配置：重启后凭证仍在，检查回到未验证，Qwen地域/业务空间须重新选择。未来任务必须将端点与模型绑定方案，不得从界面临时选择隐式继承。

## 官方路由和协议依据

查阅日期2026-10-10，仅使用官方文档确认协议；未向这些站点发送用户密钥或探测请求。

| 供应商 | 固定路由 | 凭证/返回处理 |
| --- | --- | --- |
| Claude | `https://api.anthropic.com/v1/models?limit=200` | `x-api-key`、`anthropic-version: 2023-06-01`；`data[].id` 与 `has_more`。见[官方模型列表](https://platform.claude.com/docs/en/api/models/list) |
| DeepSeek | `https://api.deepseek.com/models` | Bearer；`data[].id`。见[官方模型列表](https://api-docs.deepseek.com/api/list-models/)，本轮搜索读取到该页摘要，后续正文打开超时；认证方式另见[官方首次调用说明](https://api-docs.deepseek.com/guides/codex) |
| Kimi 中国站 | `https://api.moonshot.cn/v1/models` | Bearer；`data[].id`。见[官方 API 概述](https://platform.kimi.com/docs/api/overview) |
| xAI Grok | `https://api.x.ai/v1/models` | Bearer；`data[].id`。见[官方模型列表](https://docs.x.ai/developers/rest-api-reference/inference/models) |
| Qwen 北京 | `https://{workspaceId}.cn-beijing.maas.aliyuncs.com/api/v1/models` | Bearer；业务空间ID为单个DNS标签，服务端验证并统一小写 |
| Qwen 新加坡 | `https://dashscope-intl.aliyuncs.com/api/v1/models` | Bearer；用户显式选地域 |
| Qwen 中国香港 | `https://cn-hongkong.dashscope.aliyuncs.com/api/v1/models` | Bearer；用户显式选地域 |

Qwen 使用 `page_no=1&page_size=100&providers=qwen`，验证 `success`、`code`、页码、`output.total` 和 `output.models[].model`。字段和路由依据[百炼查询模型列表](https://help.aliyun.com/zh/model-studio/list-models)。地域对应密钥的要求见[百炼兼容接口说明](https://help.aliyun.com/zh/model-studio/compatibility-of-openai-with-dashscope)。本次不接 Coding Plan/Token Plan 专用凭证、其他地域或 Kimi 国际站；这些端点变体未当成已支持。

## 可信边界

IPC 的目标是封闭枚举，无任意 URL、厂商重定向或模型提供的地址。北京业务空间只允许1–63字节字母/数字/中划线且首尾不是中划线；不能注入路径、用户信息、端口或额外域名。枚举采用结构变体拒绝未知字段：初版普通枚举变体会忽略多余字段，合约测试暴露后已修正。OpenAI 不在这个执行供应商检查枚举中，主副指挥角色来源规则不变。

TLS客户端不读取代理配置，不跟随重定向，无自动重试。错误只映射固定摘要，不转发厂商错误正文、密钥、请求头或原始JSON。响应最多2MiB；最多解析10000项、展示200个唯一模型ID；模型名最多200字节并限定字符。拒绝回显完整密钥的模型ID。分页或展示上限显示“部分列表”，未返回的模型不能判作不可用。空列表可展示，但不显示为执行就绪。

目录来源被视为元数据，所有适配器的具体执行能力仍为unverified；清单的“部分接入/原型验证”说明实现范围，不能替代真实能力证据。API执行器、统一工作台CLI、厂商CLI、工具循环和持久任务派发仍须后续接入，不能用目录检查代替P3验收。

## 验收范围

本地回环HTTP合约覆盖七种端点配置×八种响应，共56例：成功、重定向、401、403、429、声明长度超限、非JSON、chunked超限。检查GET路径/参数、Bearer与Claude专属认证头、不重复请求和不泄露错误正文。另覆盖分页、空列表、目录/ID数量限制、恶意模型ID和端点字段拒绝。

宿主测试覆盖五家独立加密保存、重开、密文交换拒绝、删除隔离、探测期间拒绝改密钥/退出、接收通道中断清除旧模型结果，以及真实异步线程在坏密文处退出且不联网。原OMP真实离线worker测试增加活动期间替换/删除DeepSeek凭证拒绝及原密文字节不变断言。

原生验收使用隔离目录及合成密钥：五家保存和无效替换、Qwen地域/空间条件、两个完整核心重开、Kimi独立删除、Grok坏密文触发异步错误、重新保存清除旧检查。坏密文在构造HTTP请求前失败，不能描述为真实API认证测试。最终结果、日志、截图和SHA索引见第027项交接与对应证据清单。

下一步：将执行供应商纳入有持久意图、单次生成、工具调用及产物封存的worker通道，接入批准方案和独立验收；并推进用户要求的工作台CLI及Qoder/WorkBuddy既有会话产品绑定。真实官方账号、模型能力和完整P0–P6均未由本模块验收。

## 本轮结果

06:09 +08:00：328默认Rust/0失败/71忽略、5Node、fmt、严格clippy、前端/核心/桌面构建通过；另1显式离线OMP测试覆盖四路径的活动凭证保护。新原生reports/provider-discovery-native-uwFXOL与旧两槽回归reports/provider-credentials-native-F53JSe各两次完整重开通过，两Job剩余0，截图已查看。真实供应商API请求0。本轮初始失败、修正及范围限制详见第027项交接；证据reports/p3-provider-discovery-review-20261010.json。
