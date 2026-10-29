# 217 副指挥分配标识修复

216真实请求返回workerId=deepseek，但批准的执行器身份是worker-deepseek。核心严格派发检查正确拒绝；产品上下文只给了适配器目录及adapterId，原ProposedTask的workerId在转成执行配置后不再显式呈现。固定回复测试没有暴露真实模型会选择适配器ID的歧义。

## 修复

新DeepSeek Assign请求增加assignmentBindings，由批准方案的execution条目及ProjectWorkerAdapter.worker_id()推导taskId、workerId、adapterId，不使用模型自己提供的绑定。上下文说明adapterId不是workerId。该表不授权执行，原用户批准、运行授权、资源、任务状态及最终身份检查继续有效。

普通、共享API以及原生执行的应用检查按原请求选择上下文格式：旧请求没有assignmentBindings时保持原字节语义；新请求按当前/原账本作用域重新生成绑定，拒绝被改写的任务/执行器对应关系和错误用途。未改变全局context_for_snapshot，也未修改DeepSeek静态提示词、wire格式或原账本。

这是兼容性的必要条件：deputy_management/evidence.rs会根据原handoff重新计算请求fingerprint；不能直接编辑原静态提示词让既有回执失效。215的当前Codex Plan/Accept及监督路径继续复用，不添加新的发送能力。

## 验证与限制

报告入口为reports/assignment-contract-217/result.json；在该文件生成前本批仍未收尾。3项定向检查覆盖真实216错误标识的受控回放、串/并行×有/无API建议的4种完整流程，以及旧上下文/绑定篡改/用途约束；13项旧来源、配置和外部chief回归通过。模型输出受控，实际文件工具、Git、验收与重开仍执行。

新核心已通过正式CLI重开216原位置的无凭证数据，读回attention状态和2条管理记录后正常停止。原目录全部文件摘要逐项不变，无执行者请求、无凭证、无派发。

初次验证脚本直接复制目录后尝试启动，退出4；离线data-format status进一步确认reader13保护绑定原路径，正确拒绝搬移后的副本。这是验证脚本假设错误，原失败日志保留。210旧证据也明确冷备份只能恢复至原位置；因此本批保留逐字节副本，改为原路径只读重开，不改保护标记、不迁移或恢复原run。详见live/copy-start-diagnosis.json和legacy_reopen_original.py，不再运行初始legacy_reopen.py。

这些检查证明结构化绑定和执行门槛正确，不能证明真实模型下一次必然遵从。216共一次真实请求，输入5145/输出322 tokens，0执行者尝试、0重发；本次临时凭证已删除，原自有核心已正常停止。216授权已消费，修复后重试仍需新具体授权，不重用Key。

未打新安装包、未安装、未发布或审签；后续签名检查必须核对当时实际证书、产物/版本和清单。完整正式验收仍未完成。
