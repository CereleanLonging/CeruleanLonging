# 283 验收字段级诊断与一次真实差异

282一次真实调用明确被DecisionSchema拒绝，尚不能断定具体字段。其原结果冻结，未应用工作流。本轮在严格反序列化失败后，进一步输出固定原因：封包/decision对象类型、额外字段、snake_case别名、stateDigest、action、taskId、attemptId、workerId、rationale、suggestion、reason、tasks类型或缺失；不输出任意字段名、字段值、原回答或私有推理。不修正或放宽模型输出。

既有权威响应序列化和reader16继续保持。旧281读者若遇到新增的非权威诊断枚举，只将诊断显示为不可用，不影响权威账本。原279和282结果不得回填。

本地6项适配层测试包括14个字段拒绝类别、原端点/角色矩阵及旧格式；通过严格Clippy和新诊断程序构建后，按现有中转授权执行一次新的只读契约：diagnostic283、contract-sum-283、contract-attempt-283，A6API/gpt-6.1-sol，90秒、最多2048输出token、最多1次请求，无工具、代码生成、工作流应用或自动重试。新结果用于定位字段类型，不能作为原任务验收。

计划在`reports/a6api-schema-live-283/authorized-plan.json`冻结，Key仅内存/stdin，不建立凭证槽。既有279与282冻结证据保持，正式验收仍false。
