# 248 固定实验材料到完整基线方案的绑定

本轮复用239固定任务、241 seed、245材料包、246单执行者内核和247一次性执行器，新增baseline_materials及CLI compose。没有重做238桌面发送、232编程、OMP196、WorkBuddy203/206或真实218闭环。

## 实现和边界

RunSpec增加schema2材料请求；schema1原字节及digest保持，可只读检查历史记录。新的CLI prepare只接受schema2；生产run只接受已经验证且声明real-attempt的材料。库内schema1准备兼容旧夹具，但不能进入真实API执行。

compose核对调用方固定的清单字节摘要、固定239 schedule、slot、task正文、seed文件和验收规范；严格解码配置，固定执行者模型、端点、文件工具及请求/时间/输出等参数。它从材料重建整个PlanRevision，再比较完整spec摘要，避免只核对某几个字段。任务和工件范围、隔离工作树键、权限、验收器参数和超时均在方案里。文件工具只给执行者读写权限；RunCommands用于独立验收，未给模型新增命令工具。

launcher/checker为固定来源摘要，同时验证launcher实际相对路径解析到的checker；Node、Git和验收输入均使用现有PinnedCheck。项目、数据、材料、目录池、受信任工具分离。Git通过原有受限进程读者检查仓库身份、真实commit及原始blob，不能只填一个形似commit的字符串。固定seed保持在项目中，reference/mutant内容不进入准备材料；12份项目均由245/241已有seed创建。

prepare在新数据根产生前完成核对；fresh在启动意图、凭证读取和API之前重新检查材料及准备证明。任务开始后仍沿用原工作树seed核对和验收器固定输入检查。历史inspect仅核对原记录，不要求外部材料永久在线，不恢复、派发或重跑验收。材料证明保存在prepared.json并由回读输出明确标为准备时证明，不是第三方真实性认证。

材料包在prepare/run之前要求attachments为空，额外文件和空目录均拒绝；运行后可保存关联证据，但不得将同一实验再次启动。已有Git目录池须预先初始化。工具仍是开发CLI，未接RuntimeHost/UI，也未进入0.1.15安装包。执行器的既有计时边界保持：材料准备/复核在启动计时之外，工作树/执行/独立验收在内；团队后续必须采用相同边界另计人工时间。CLI未新增交互取消。

## 检查与保留问题

首轮编译缺少ApiWorkerSelection导入，编译日志和源码快照保留；补导入后构建通过。6组Rust检查全部通过：12任务方案构造、12类完整方案变化、12类材料/仓库变化、准备后材料变化在意图/凭证前停止、旧格式/合成材料禁止进入生产，以及S01完整合成执行与重开拒绝。

S01通过真实Git工作树、文件工具、固定Node验收器和Accept：2条合成执行者回复、0管理请求、1次独立检查；两次重开0重发。它是合成夹具，不计真实效率或真实调用。12份构造通过不等于12任务都已产品执行验收，未覆盖的运行分支仍待补。

8项CLI负例确认重算清单后的错误正文/seed、错误slot/worker摘要、未知配置字段、声明不符、额外目录和错误checker位置均被拒绝。复查发现schedule普通DOS路径和规范化的Windows扩展路径混比，会漏判schedule位于可变目录池；用只读compose复现后保留原结果，统一规范化修复。原六组检查使用修复前独立二进制，最终程序只补路径差异、合法材料、有效但内容错误的commit和原结果回读，没有重派S01。最终fmt及core全目标严格Clippy通过。

另以明确不存在且始终未创建的凭证目录检查最终生产入口：测试材料声明real-attempt，只验证缺凭证失败路径，使用本地夹具批准，不代表获得真实调用授权。意图落盘、失败终态、0 API请求和重复拒绝均确认。此失败目录不得自动重试，不可列入真实72槽。没有读取用户Key、没有访问模型。

247原数据和248已完成原数据的只读前后摘要相同。全部捕获进程退出、日志关闭，命令数量和实际证据见result.json。build.py首轮失败保留；其他已执行脚本均结束，不要重跑或覆盖输出。最终代码用source-final.json，六组测试对应source-fixed.json，原始源码source.json保持。

## CLI

```
baseline_experiment compose <request.json> <request-file-sha256>
baseline_experiment prepare <new-data-root> <schema2-spec.json> <spec-file-sha256>
baseline_experiment inspect <data-root> <spec-digest>
baseline_experiment run <data-root> <spec-digest> <approval.json> <approval-file-sha256> <credential-root>
```

compose输出spec/materialProof和authorized=false，不批准、不派发。真实run仍需具体模型/预算/范围的用户批准及本机凭证；本轮未取得新的真实调用授权。

## 下一步

补团队模式的同口径启动前来源库存/计时及材料绑定，并复用240接线覆盖其余必要任务运行分支；随后才冻结真实模型、单槽/整体预算和具体有限调用计划。不要再从零实现基线或重跑已通过接入。RuntimeHost/UI衔接、供应商/桌面完整故障矩阵、严格桌面原任务监督、生产金额门禁、安装GUI/升级回退、独立Windows和正式签名仍在需求表中。用户已说明暂无独立环境/正式证书，不重复索要，也不擅自新建密钥或修改信任。

真实效率0/72，releaseAccepted=false、目标active。238已通过、Qoder可最小化；没有新模型、桌面动作、用户凭证、打包、安装或信任变更。237的0.1.15包清单保持，不包含本轮实验源码。账户额度不可可靠读取，95%安全收尾规则保留。

权威入口：[latest](../reports/material-binding-248/latest.json)，[结果](../reports/material-binding-248/result.json)，[完整需求](../reports/material-binding-248/requirements.json)。
