# 281 管理响应拒绝阶段与兼容记录

279已证明A6API规划、分配、编程及15项独立检查通过，但最终Accept无有效决定。原响应没有详细拒绝阶段，不回填原因、不重发原操作。

本轮接收端增加固定阶段：Billing、Wire、ToolCall、DecisionJson、DecisionSchema、StateDigest、Role、DecisionValidation、SupervisionTarget，以及Accepted（仅表示适配器决策封包有效，不代表工作流已验收）。不记录原始回答、私有推理、自由格式解析错误或Key。

`DeputyResult.decode_stage`只在内存传递并跳过序列化；权威账本的字段、摘要及reader16保持。核心先保存原响应，再尽力发布`management-response-diagnostics/<operation>.json`，以原请求、原结果及响应摘要绑定、限制4096字节、独占创建。旧程序忽略此独立目录。诊断缺失/损坏不改变账本恢复、用量或重发权限；当前管理详情页与CLI现有message字段呈现阶段，旧记录明确显示无法追溯。

验证与结果以`reports/management-diagnostics-281/latest.json`为准：适配层覆盖9种阶段和旧序列化格式；账本覆盖有效/丢失/篡改/半写/旧记录，重开与禁止重发；真实宿主受控回归检查详情及退休提交后的退出。所有本地模型响应为受控夹具，不计为真实模型验收。

下一次真实差异仅做新的只读Accept契约诊断，最多一次A6API请求。使用新的诊断项目/任务身份，不应用工作流、不重发279、不重新编程，不把诊断有效计为279通过。具体见282计划。280源码修复尚未加载进原PID40100；保持原现场，不能强杀、篡改账本或编造决定来退出。
