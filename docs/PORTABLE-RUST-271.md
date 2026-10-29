# 271 产品Rust便携证据CLI与跨实现核验

2026-10-23T03:07:25.764955+08:00。新增core::portable_evidence，沿原backup::open保留目录/文件句柄、拒绝重解析点及硬链接，沿原禁止覆盖的publish发布最终清单；格式保持270 v1。新增工作台CLI的evidence-archive create/verify，在原离线操作分支完成后返回，不启动或连接Host，不读凭证，不执行载荷。

## 使用

创建时--data-dir是冻结索引中相对路径对应的源证据根；目的地须在其之外且不存在：

```powershell
cerulean-runtime.exe --workbench --data-dir <源证据根> evidence-archive create --index <冻结索引绝对路径> --index-sha256 <另存索引摘要> --to <新的绝对目录>
```

核验只读--from，不访问或创建--data-dir；例如本轮独立程序可核查迁移包：

```powershell
& "D:\test\cerulean-portable-271\standalone\cerulean-runtime.exe" --workbench --data-dir "D:\test\cerulean-portable-271\must-not-create-data" evidence-archive verify --from "D:\test\cerulean-portable-271\rust-relocated" --manifest-sha256 9cce40792cf0196453d929ec079e29d81779eb35d931696e098cad6fd9f0b785
```

当前程序SHA256：b9f6b44d460a63f3c3a53444981027139ecfc7985fb2d6b7c51ec8321de92119；Windows现有运行库仍需可用，未宣称新机器已验证。

v1清单/README与Python兼容，外部预期摘要仍必需；仅允许相对常规路径、有限文件/大小，清单不得声称executionAllowed/restorableWorkbench/realityAttested/releaseAccepted为true。Rust额外明确拒绝Windows设备名称和目录大小写别名。源内容变化或复制中断保留pending，不覆盖或自动修复。

## 实际验证

- 5项Rust库检查覆盖迁移及原源路径消失、篡改/缺件/多件/空目录/pending/伪授权/重复JSON字段、危险路径/凭证名/大小写冲突/布尔大小、源变化留pending/不覆盖、真实硬链接/junction及文件写锁。1CLI解析检查确认必要摘要和不接受执行选项。
- 真实CLI先读取270原Python完整归档；从同一269冻结索引创建新Rust完整归档，独立Python270验证器核验通过；复制到另一目录后，仅复制的Rust程序核验通过。各次完整1334文件/90601251字节，不调用Python作为Rust后台。
- 特意传入不存在的--data-dir，核验后仍不存在；原269文件保持。错误预期摘要和已有目标覆盖各实际exit3、archiveRejected，未再写入。共6个CLI/交叉工具调用，原目录不可重跑或覆盖。
- 首次Clippy发现离线枚举的不可达IPC分支未列新变体，修复后严格all-targets Clippy通过；原错误保留。fmt、core/CLI测试构建及生产core开发构建通过。14捕获回执均终态，包含原编译失败及2预期拒绝，输出关闭摘要核对。无模型/Key/常驻核心启动。

原Rust归档D:\test\cerulean-portable-271\rust-original；迁移副本D:\test\cerulean-portable-271\rust-relocated。source-before/ready/verified、测试程序和binary固定，270的40项原证据保持。原0.1.15/reader13发行清单93b8ac9b76772d24ae0af1e43dfa899094d7d03ae3e2a3e78c744b729573e51f保持；开发reader15，未打包/安装/签名/创建密钥或改变信任。

## 剩余范围

- 产品Rust库及CLI已接，桌面归档/核验入口仍待；新代码仅开发程序，尚未进入正式签名发行包或安装验收。
- 本轮跨实现与另一目录核验在同一Windows进行，不能当跨OS或干净机器通过。归档只证明所选字节和格式，不能重放业务账本、证明真实模型/费用/收益或恢复工作台。
- 拒绝已知凭证文件名不等于通用文本脱敏，源代码和日志可含原路径及用户内容；本轮未外发。清单预期摘要须独立保留，不是发布者签名。
- 完整首批/CLI/桌面故障矩阵、严格原桌面任务监督、真实金额、真实效率72槽、候选当前证书/签名/版本/清单和独立Windows安装仍未整体签收。

目标active、releaseAccepted=false、真实效率0/72。238无需前台/重发。账户用量不可可靠读取，可靠95%账户信号到达安全收尾，不用上下文占用替代。
