# 225 Qwen Token Plan 接入准备与路径纠正

用户确认：阿里云 Token Plan，模型 `qwen3.8-flash`，目前没有 DeepSeek 副指挥凭证。随后提供 `C:\Users\姚\AppData\Local\Qoder\Qoder Launcher`，明确希望通过现有 **Qoder** 使用该模型。因此本轮实际路线是 Qoder 现有桌面接入，不安装或替换为 Qwen Code。

## 已核对与继承

- GPT 已有真实 Codex/ChatGPT 登录路线证据：[原执行记录](../../workbench-probes/reports/commander-execution-1791118304583.json) 与对应 review。原主、副指挥及 worker 为 gpt-6-astra，16项检查。旧产物 SHA256 与报告一致。产品 P2 Codex 编程也已通过，但其中管理决策为合成。不能把这些结论写成 GPT API Key 路线真实通过。
- Qoder 0.4.3 原 GLM 20/20、旧 Qwen 失败保留，见 `PROTOTYPE-REUSE-BASELINE.md`。旧模型成果不直接证明本次 qwen3.8-flash；复用桌面绑定/独立验收机制，仅补实际配置差异。
- 官方 Qwen Code 配置列明 Token Plan 的 qwen3.8-flash，端点 `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`。这不是 Coding Plan 端点，也不是工作台既有 Qwen 普通 API 端点。
- 官方 Token Plan 个人版说明限定编程/智能体工具内交互式使用，排除自定义后端和非交互批量脚本。因此不能直接将该套餐 Key 接到现有无人值守 API worker，也不能用 CLI 包装把自动批处理冒充交互。
- `scripts/inspect_qwen_token_plan.py` 是已准备的 **Qwen Code 专用**只读配置检查工具，4项合成测试通过，未用于用户真实配置。不会调用供应商或输出密钥；单文件检查不证明合并配置、凭证有效性或真实编程能力。用户现已选择 Qoder，该工具不适用于 Qoder，不作为其通过证据。

## 官方来源（2026-10-20读取）

- https://help.aliyun.com/zh/model-studio/qwen-code
- https://help.aliyun.com/zh/model-studio/token-plan-personal-overview
- https://help.aliyun.com/zh/model-studio/token-plan-overview

## 当前插入任务：Qoder迁盘

用户请求确认 C 盘占用并迁到 D 盘，随后授权正常关闭、迁移、重新启动。只读目录统计：C盘 `.qoder` 867.87 MiB（不含联接目标），Local 启动器0.11 MiB，Roaming约0；E盘主程序2758.87 MiB，`E:\qoder-data` 992.13 MiB。主进程实际 `E:\千问\Qoder\.qoder-versions\0.4.3\Qoder.exe`，并非整体在C盘。

迁移目标 `D:\Qoder`，旧路径保持目录联接兼容，关闭后逐文件校验再切换，原配置及聊天不输出。结果以后续迁移报告为准；本文不是已迁完声明。

## 续接

1. 完成迁盘与正常拉起；不自动发送模型任务。
2. 从原型/产品旧证据复用Qoder只读绑定，核对现有可见会话、实际模型/套餐配置，不能将 launcher 版本当主程序版本。
3. 准备隔离 `sum.mjs` 编程任务、独立验收与具体交互发送范围。不能复用已消费的 WorkBuddy/DeepSeek 授权。
4. 没有新的 DeepSeek 凭证，不开始真实三级链。自动调度适用范围未核实，不列为通过。

224修复已保留，冻结0.1.13不含该修复；正式验收未完成。当前无可靠账户用量遥测，不能宣称95%自动监控生效。本轮截至本文写入0真实模型调用、0凭证使用。
