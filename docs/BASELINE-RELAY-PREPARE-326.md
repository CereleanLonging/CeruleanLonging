# 326：新建 A6API 基线项目的 reader16 准备

2026-10-24。修复已合入开发源码，未进入 315 冻结包。

原单执行者基线入口已有实现和 DeepSeek 验收。277 新增 A6API 后，新建基线数据根仍没有 reader16 保护，首次 Propose 因此必被拒绝。这是新增提供方与新根准备的组合缺口，不改写旧基线成功结论。

现在只对本次原子创建的新 A6API 根，在写 RunSpec、管理目录或工作流之前，复用真实冷备份、审阅和晋级流程建立 reader16。冷备份位于同级 `data.reader16-cold-backup`，内容精确为初始空 events 和 writer.lock；已有根或同名备份一律保留并拒绝复用。普通 DeepSeek 不建立该保护或备份。

释放与重新取得 writer 锁之间，分别检查备份精确清单和晋级后的原根、事件、保护记录摘要。若另一 writer 插入内容或替换保护记录，停止准备并保留全部材料。没有新 guard 格式，没有放宽原备份、SID、路径或 reader 规则。这份空根备份是保护来源，不能当作运行后完整数据的恢复点。

## 实际验证

- 7 个新测试函数通过，覆盖正常 A6API、普通 DeepSeek、已有备份/根、保护缺失以及五种确定并发交错。
- 正式 prepare → 合成两轮文件执行 → 独立验证 → Accept → 两次回读/拒绝重执行已通过；与其他相关用例合计 35 passed / 19 ignored。
- 严格 adapters/core all-targets Clippy 和 fmt 通过。原 Clippy 失败仅为管理测试辅助函数的 dead_code，已用 cfg(test) 限定，未抑制警告或增加原业务链读取。
- 324 原基线测试的历史失败原因仍未明；新成功不宣称修复该失败。诊断和保留现场机制已加入。

另已用固定 315 旧核心完成本次合成基线的冷备份验证和原路径恢复，当前 reader 恢复后关联检查通过，见 [324 结果](../reports/gateway-id-324/final-result.json)。这不证明旧 Host 对该基线的业务语义回读。

没有真实模型、Key、安装、签名或信任变更。产品 UI/IPC 的完整 A6API 实验导入及 72 个真实效率样本不能由这些合成检查代证。

详见 [独立审阅](../reports/baseline-relay-prepare-326/post-merge-static-review.md)、[合入记录](../reports/baseline-relay-prepare-326/published.json)、[原始回执](../reports/baseline-relay-prepare-326/checks/)及 [324 统计更正](../reports/gateway-id-324/diagnostic/results.json)。
