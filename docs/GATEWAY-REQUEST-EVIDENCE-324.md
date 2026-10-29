# 324：A6API 网关请求身份留证

2026-10-24。已合入开发源码，未进入 315/0.1.22 冻结包，正式验收仍为 false。

325 对原账单详情的只读核查取得网关 Request ID，但原调用只保存正文 response ID，两者没有提供方映射。本修改为今后的 A6API 请求保存固定 `X-Oneapi-Request-Id` 响应头；不会补造旧调用的头，也不代表金额核销完成。

实际 API 文件执行、基线执行和管理线程在收到响应头后留证，原结果发布后再关联。辅助文件位于原 journal 之外，绑定原意图及原结果摘要；本地请求 ID、正文 ID、网关 ID 分开保存。原事件格式与 reader16 不变。缺头、拒绝值、文件失败及只收到头的情况均不给重发、解除 Unknown 或结算权限。

值有长度和字符限制，重复头及凭证反射被拒绝，不保存其他头或错误正文。每个请求至多两个 4KiB 辅助记录。同步持久写入可能增加正文读取和取消延迟，不能承诺完全无时序影响。当前管理产品路径负责写入；管理辅助读取函数仅供合成检查，尚无管理网关 ID 的 UI/CLI 查询入口。

## 已取得的检查与保留的问题

- adapters 50 项通过；核心增量 15 项通过。
- 首次相关核心回归为 86 passed / 1 failed / 30 ignored。唯一失败是基线 `terminal.accepted`；当时未打印细节，现场未保留，根因仍未知。
- 加入仅测试诊断及失败现场保留后，精准复查 1 passed，同模块 10 passed / 1 ignored；这些成功不能解释原失败。
- 合入 326 后，正式 prepare、执行、Accept、两次回读及关联检查所在批次 35 passed / 19 ignored，严格两 crate all-targets Clippy 和全工程 fmt 通过。原 Clippy 的 test-only helper dead_code 失败及修正回执均保留。
- API、管理各一个固定新目录的合成导出已通过，没有真实模型请求或凭证读取。旧包首次检查在启动前遇到脚本长路径误报，原数据与备份一致；修正后仅续接原导出，失败回执保留。
- 固定 315 核心已验证两类冷备份并恢复到原路径，管理记录在恢复前后均由旧核心实际回读。当前 reader 随后核对原结果及关联：API 4 个、管理 2 个辅助文件保持，两个本轮管理核心正常退出。原包、原始记录和归档未改。

累计 156 个不同 Rust 用例有通过证据，30 个被跳过的用例没有本轮通过证据；历史失败仍未明，不能称全部历史批次无失败。另有 v4 驱动的 12 项受控路径检查。不能把语法预检、编译或重复执行累加成新的测试能力。旧包对 API 账本的业务语义回读仍是单独缺口，备份恢复不能代证。

## 接续材料

- [实现与静态审阅](../reports/gateway-id-324/independent-review.md)
- [29 文件独立源码快照](../reports/gateway-id-324/source-after-checks.json)
- [原检查和失败回执](../reports/gateway-id-324/checks/)
- [诊断结果及统计更正](../reports/gateway-id-324/diagnostic/results.json)
- [旧包兼容准备](../reports/gateway-id-324/compatibility/PLAN.md)
- [长路径现场核对](../reports/gateway-id-324/compatibility/long-path-diagnosis.json)
- [本轮结果与原始回执索引](../reports/gateway-id-324/final-result.json)

原 295 历史身份缺口、284 Unknown 和金额预算保护保持。正式签收仍按 318 完整门槛，不能用本项局部通过替代。
