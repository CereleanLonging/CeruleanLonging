# 355：否决两个弱访问目录pin替代方案

2026-10-25。只测343未覆盖的DesiredAccess差异，复用原Win32 helper，不重跑328原exact、不启动产品/OMP/模型，不改生产源码。事前容量检查通过，预留1MiB，原生实验新增约34KB。

| DesiredAccess（均share1/flags0x02200000） | pin在位时子文件rename | 目录自身rename/delete | 与write/share7持有者共存 | 结论 |
| --- | --- | --- | --- | --- |
| GENERIC_READ | error32 | 两者error32 | 拒绝error32 | 保持原保护行为 |
| FILE_READ_ATTRIBUTES | 成功 | 两者成功 | 成功 | 不适合作原安全pin直接替代 |
| 0 | 成功 | 两者成功 | 成功 | 不适合作原安全pin直接替代 |

每种模式采用全新独立目录；pin前与释放后均用新文件对作对照，均成功，没有重试原失败动作。75条原生事件记录操作发生时对应pin仍存活、返回码和句柄关闭，全部句柄正常关闭。子代理独立复核脚本、事件和摘要后确认否决结论。

元数据身份可读不代表目录路径仍被保护。不能因为子文件发布成功，就用上述变体替换crates/core/src/omp_file_tools.rs的pin_directory；否则会削弱原路径绑定边界。当前生产实现保持不变，已有343派生索引回退证据保持。

本结论只否决所测两个直接替代模式，不解释328原共享冲突持有者，不证明所有文件系统行为，不等于生产缺陷已修复或正式验收通过。后续需历史持有者因果证据或不同的完整安全设计；不要再次重复本矩阵或重跑旧exact求通过。

证据：[原生结果](../reports/omp-pin-access-355/result.json)、[独立复核](../reports/omp-pin-access-355/independent-review.json)、[原始事件](../reports/omp-pin-access-355/events.jsonl)。
