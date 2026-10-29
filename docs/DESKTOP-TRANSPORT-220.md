# 220 WorkBuddy现有会话监督：新增入口排查

当前安装为5.8.3，app.asar SHA256仍为`3e50fcb5b060deba4f1ea9a8c293cf955bf12d7e52d971985de914350afd9077`，与207完全一致。207两层queueSendNow不传原请求约束的7项受控结果继续有效，本批没有重跑这些测试，也不重发203编程任务。

## 新检查的入口

从同一安装包读取main/transport、daemon入口、eval-inject-server、main-bootstrap及preload接口元数据。只提取代码到reports，不加载厂商模块、不连接进程或端口、不读取聊天/凭证、不改变安装文件。

| 入口 | 本次新证据 | 结论 |
| --- | --- | --- |
| 评测代理POST /chat/:session | handleChatContinue只提取prompt，driveChatTurn调用普通SEND_MESSAGE；传入旧或当前expectedRequestId均不转发该约束 | 不能证明只进入批准的原请求，不可用来替代严格steer |
| 评测代理POST /sessions/:session/inject | 解析user/assistant/tool历史，调用session/inject_history；忽略额外expectedRequestId | 写入聊天历史，不是原任务运行中建议；不得伪造消费回执 |
| 评测代理启动 | main-bootstrap.startEvalProxyServer检查evalModeEnabled；未启用则返回 | 安装包含代码不证明当前已开放，不为本批改变启动模式或重启应用 |
| preload队列导出 | 导出queueSendNow等队列方法；207已证明其到ConversationBridge的调用不保留原请求约束 | 与旧队列排查合并，不重复发送验收 |
| WB HostDispatcher | 调用分发具有accept/resolveContext等接收端检查 | 本批不绕过来源检查，不把通用分发器当成已授权的任意方法入口 |

新增5个受控场景使用安装包原方法与合成服务：旧/当前expectedRequestId下均走普通发送、缺失会话拒绝、历史写入路径、非法历史输入拒绝。方法提取摘要与原文件SHA保留，原应用调用/网络请求/模型调用均0。受控检查证明所选入口不满足要求，不证明所有可能接口不存在。

## 后续最小条件

仍需找到厂商支持的现有会话入口，能获取并在接收端原子核对sessionId及expectedRequestId，明确禁止变成新prompt；或提供可安装、受支持、权限明确的扩展入口。随后才能实现一次性投递及原消息消费观察。不能通过改厂商安装包、伪造历史、另起CLI任务或取消后重开来冒充当前任务监督。

在这些条件出现前，保持桌面主动监督未验收。普通编程及独立验收继续继承206，API三级链继承218，OMP原任务建议继承196；不能因桌面监督缺口抹去这些成果。后续核查应先看当前文件摘要，有变化才重做相关差异检查。

本批未审签、打包、安装或发布，不引用207旧签名审计作为当前正式签名结论。完整首批矩阵、可靠性/效率、新候选升级/签名与独立环境验收仍需推进。账户额度没有可靠实时读数，不能声称95%阈值自动监控有效。
