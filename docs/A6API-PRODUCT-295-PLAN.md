# 295：A6API / 0.1.19 单任务编程差异实测预案

状态：**仅准备，尚未调用供应商**。用户 2026-10-23 已明确批准中转调用、直接实测及必要操作。本计划将该总授权收束到一次全新隔离任务；不得复用或重发 279、284、286 的原请求。执行前先核对 [293 冻结清单](../reports/p6-windows-package-0119-20261023/manifest.json)、本计划、`reports/a6api-product-delta-295/verifier-preflight.json` 和准备后的 `live/authorized-plan.json` 摘要。

使用 293 冻结的 `cerulean-runtime.exe`（0.1.19 / reader16，清单核心 SHA-256 `aa5cbf4fd98dace69ef6cd48ef45368f8a1fcc7224c279b6034721530d3f8af0`），不用可能被并行开发改动的 `target`。新数据目录 `reports/a6api-product-delta-295/live/data`，新项目 `relay295`，唯一产物 `live/project/product.mjs`，初始为明确未实现的占位函数。任务是实现 `product(values)`：只接受数组内的有限原始数字，空数组返回 1，按顺序相乘且不修改输入；非法输入抛 `TypeError`。模型只能读/写该文件，写入必须带实际原 SHA；不能运行命令、联网、改其他文件或触碰独立验证器。

主指挥、副指挥和执行者均使用 A6API `gpt-6.1-sol`；最多 **3 次管理请求 + 4 次执行请求 = 7 次供应商请求**。执行者最长 90 秒、最多 3 次文件工具调用，单次管理 90 秒。先核模型生成的计划恰为一个无依赖任务、指定执行者与唯一产物，再登记批准；副指挥分配，工作台独立运行 28 项隔离验证，主指挥收到结果后 Accept。成功后正常退出、删除本次临时 DPAPI 凭证并两次重开，只读比较数据逐字节不变。任何失败、未知或超时均停止该计划、不自动重发；清理期间不强杀仍在执行的核心或用户进程。

凭证只通过执行器私有 **stdin** 单行输入，再经产品 `credentials set` 的 stdin 写入隔离 DPAPI 槽；不放在命令行、环境变量、源码、报告或日志。脚本不读取历史聊天或旧配置。286 的历史取 Key 脚本绑定旧会话问答，虽然本次授权覆盖中转实测，它不证明同一旧 Key 当前有效，也不应机械复制到新执行器。凭证来源由执行时的受控输入端确定，执行前的准备和预检一律不读取明文。

**请求次数不是金额上限。** A6API 的报价身份、缓存语义和逐笔账单尚未确认；该模型的产品金额门禁仍关闭，本任务不声称费用预留、实际金额或 72 个效率样本。真实调用后只记录响应 usage，缺失字段保留未知，`actualMoney=null`。现有账户额度无法可靠实时读取，95% 停止规则仍有效；不能声称自动监控有效。官方 OpenAI Key 实测继续延期。本轮不安装、不签名、不新建密钥或改变信任。

执行入口依次为 `python reports/a6api-product-delta-295/preflight.py`（离线）、`python reports/a6api-product-delta-295/prepare.py`（本地工作台，无模型）、随后在确认冻结计划摘要、提供受控 stdin 后才运行 `python reports/a6api-product-delta-295/execute.py --live --plan-sha256 <摘要>`；成功后运行 `finalize.py` 只读审计。`execute.py` 的 `execution-entry.json` 是不可重跑标记；不能因异常删除它再执行。286/293 原材料不可改写，正式验收仍为 `releaseAccepted=false`。
