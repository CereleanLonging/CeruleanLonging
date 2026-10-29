# 本机供应商凭证配置

2026-10-10 第027项更新：固定凭证枚举现扩展为 OpenAI、DeepSeek、Anthropic、Qwen、Kimi、Grok，分别按供应商及工作台目录加密；旧两家密文保持兼容。五家执行供应商增加异步模型目录检查，OMP运行中和相应目录检查中禁止替换/删除对应密钥。模型目录成功不代表生成或工具能力通过，详见 [首期API目录实现](PROVIDER-DISCOVERY-V1.md) 和最新交接。下文保留最初两槽版本的历史证据，不再表示当前仅支持两家。

2026-10-10。DeepSeek独立凭证槽已接入可信核心、IPC和桌面接入管理。保存仅证明加密文件存在；OMP产品任务尚未从该槽启动，API认证和编程能力仍待验证。

固定枚举允许openai和deepseek，不解释为任意路径或远端地址。SetProviderKey和RemoveProviderKey不生成工作流或模型请求。OpenAI入口复用原检查中和指挥请求中禁止变更的约束；接OMP异步任务时还需增加DeepSeek活动任务限制。

DPAPI分别保存credentials/openai.dpapi和credentials/deepseek.dpapi，绑定供应商及工作台目录。OpenAI旧文件名及熵参数保持；独立使用旧literal namespace生成的密文仍可解密。交换两家密文或复制到另一目录均拒绝。

providerCredentials快照只返回固定供应商与keyConfigured。表单使用密码输入，保存后清空；无效替换保留旧密文，删除DeepSeek不影响OpenAI。SecretInput和解密内存沿用归零处理。

完整检查reports/p3-provider-key-full-checks-20261009.log：236Rust通过、41专用或真实入口忽略，4Node IPC/服务测试、契约生成、前端和核心构建通过。最终all-targets/custom-protocol严格clippy与原生桌面构建于10日通过。新增三个测试覆盖槽隔离、无明文历史、替换删除重开、密文交换/跨目录及旧OpenAI兼容；原两项测试继续通过。

真实原生reports/provider-credentials-native-lL89zg/report.json通过：保存、输入清空、无效替换保留原密文、正常替换、两次核心完整重启及独立删除。扫描产品数据无合成密钥明文，快照无秘密，未生成计划或管理请求。三份截图保留，已查看保存及删除截图确认标签和输入无裁切。合成凭证限定在隔离目录，API认证和模型调用均为0。

首次原生启动受沙箱spawn EPERM阻止，p3-provider-key-native-20261010.log保留；正常权限重跑通过，p3-provider-key-native-authorized-20261010.log。初次Python默认GBK读取失败未改文件，明确UTF-8后完成编辑。所有本批已知句柄结束。

后续先接OMP审核工具代理，再将凭证、进程与持久桥接纳入RuntimeHost异步任务及用户取消恢复。真实凭证只由用户在产品本机配置。首批其他API/CLI、真实OpenAI指挥链、Qoder/WorkBuddy已有会话、并行集成/效率和P6范围保持。未重跑发布包/干净机器安装，完整P0–P6未通过；旧验收入账间歇失败根因待查。

证据reports/p3-provider-key-review-20261010.json，所列源文件副本reports/p3-provider-key-sources-20261010，仅所列文件而非完整构建归档。
