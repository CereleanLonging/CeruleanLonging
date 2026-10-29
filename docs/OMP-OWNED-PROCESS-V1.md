# OMP 受控本地进程

2026-10-09。承接 [持久记录与工作流桥接](OMP-JOURNAL-BRIDGE-V1.md)。`omp_process::WorkerProcess` 将真实 stdio 接到既有 WorkerTransport，供可信宿主使用；尚未接到 RuntimeHost 的异步任务和桌面入口。

## 生命周期与启动配置

Windows 创建进程时，通过 PROC_THREAD_ATTRIBUTE_JOB_LIST 原子加入 KILL_ON_JOB_CLOSE Job；不是启动后再补绑定。复用独立校验器现有 handle、属性列表、管道和参数转义实现，新 `verifier/process/interactive.rs` 提供交互输入。继承句柄仅为指定的标准输入输出，禁止 shell，后台隐藏运行。

启动前核验 WorkerSelection、可执行文件摘要、路径及重解析点；保留禁止写入/删除共享的文件句柄到 owner 结束，防止运行中替换批准的二进制。每次必须新建独立 profile，拒绝含项目配置或 .env 的祖先目录。运行目录位于 profile/workspace，独立 HOME、USERPROFILE、APPDATA、TEMP、agent、sessions 和禁用重试配置；不从用户项目加载隐含规则。

环境块只含 Windows 加载变量、隔离目录与明确传入的 DeepSeek 密钥。密钥使用 SecretInput，原生 UTF-16 环境块用完归零；不放入命令行、配置或产品事件，stderr 仅丢弃并累计字节数。初始支持 DeepSeek 的显式模型选择，其他供应商还需各自映射。当前没有读取系统或其他应用的凭证；本批只使用公开标记为无效的合成占位值。

初始握手要求实际 session/provider/model、零消息、零工具、没有流式/压缩/后台工作/队列且已静止。通过后才开放一次执行通道。关闭先释放 stdin，等待整个 Job 最长五秒；仍有主进程或后代时终止整个 Job，再确认 ActiveProcesses=0 和父进程退出码。owner 被正常丢弃或宿主突然终止，Job 生命周期也会清理进程树。代码不把进程退出当作任务成功；任务结果仍由 journal 和工作流处理。

这是 Windows 进程树生命周期管理，不是文件系统或网络权限沙箱。当前 OMP 原生工具、扩展、技能、规则、LSP 等均禁用，尚不能执行编程工具。后续通过显式审核工具代理接用户项目。第三方程序自己的诊断文件策略还需在真实凭证/工具集成时审查，不能由产品仅存 stderr 字节数推断全部第三方日志均完成脱敏。

## 当前验证

真实 OMP 18.4.4：`reports/omp-owned-readonly-20261009-212305.json`，PID 19604，negotiate_protocol 和 get_state 各一次，provider=deepseek、model=deepseek-flash，退出码0、未强制终止、剩余进程0、stderr字节0。零prompt，没有读取真实凭证。隔离目录 `D:\test\omp-owned-probe-20261009-212305` 保留；该证据只证明受控进程握手/关闭兼容，不证明模型认证或编程能力。

离线辅助程序 `examples/omp_offline_worker_fixture.rs` 不包含网络调用，只接受指定合成凭证与 fixture 模型，实际检查禁用参数及环境清理。`tests/omp_owned_process.rs` 的三项显式测试通过：

- 正常完成和真实journal写入；收到prompt后退出/丢ACK保持Unknown；错误模型不能取得执行通道；运行中二进制不可写、结束后释放，旧profile不能复用。
- EOF后仍存活的真实后代由Job强制清理，报告forced=true和零剩余；直接丢弃owner也使父进程和后代句柄都成为已退出。
- 父测试实际结束拥有worker的宿主测试进程，不执行其Drop清理，仍确认OMP替身及后代退出。不是用模拟返回值代表进程终止。

这些测试由已构建的固定辅助程序路径和 D:\test 隔离父目录显式启用。普通cargo test忽略三个专用测试及一个宿主辅助入口，显式执行日志为 reports/p3-omp-owned-process-crash-20261009.log。临时证据由测试清理，日志和源码是留存依据。早一轮两项测试日志也保留。

## 下一步及验收限制

受控进程与持久化桥接各自可用，尚须在同一生产 RuntimeHost 任务生命周期内联通；目前只读探测不派发，离线进程测试调用 journal 执行入口。下一步接产品凭证存储/选择和审核工具代理，再接异步调度、取消、结果详情、未知处置及实际编程验收。原始产物封存和工作流提交之间的实际终止矩阵也仍待完成。

OpenAI主副指挥真实全链、其他首批API/CLI、Qoder/WorkBuddy已有会话、并行集成、效率及P6安装升级范围全部保持。OMP能力目录未提升为已验证，完整P0–P6未通过。

## 本批收尾（21:24 +08:00）

最终工作区233项常规Rust通过、41项真实调用/显式辅助入口忽略；三个OMP实际离线进程测试另行显式执行并通过，不重复计入常规233。最终fmt检查、all-targets/custom-protocol严格clippy及示例编译通过。启动前增加运行时存在检查，避免在没有Tokio运行时的调用中启动进程后panic；之后重新构建并完成三项显式回归和真实只读探测。初版只读证据211909保留，最终212305对应收尾构建。

本批未重跑前端/Node、原生界面、发布构建或安装；既有验收入账间歇失败本次未复现、根因仍未确认。所有本批测试/探测句柄结束。证据索引reports/p3-omp-process-review-20261009.json，所列源文件副本reports/p3-omp-process-sources-20261009，不是完整构建归档。

## 2026-10-10：部署目录审查发现的上下文隔离遗漏

本节补充前述历史记录；后台、调度和任务UI的后续进度见 [OMP后台记录](OMP-BACKGROUND-V1.md)。普通用户数据目录部署仍未验收，不能删除祖先目录检查来让启动通过。

核查固定版本 `v18.4.4` 公开源码发现，`--no-rules` 只设置 `options.rules = []`，不设置 `options.contextFiles`。SDK仍独立调用上下文发现。`agents-md` 和 `claude-md` 提供器分别向上搜索 `AGENTS.md`、`CLAUDE.md`；隔离 HOME 与 workspace 为同级目录时，不能依靠 HOME 自动截断工作目录的向上搜索。依据：[main.ts](https://github.com/can1357/oh-my-pi/blob/v18.4.4/packages/coding-agent/src/main.ts)、[sdk.ts](https://github.com/can1357/oh-my-pi/blob/v18.4.4/packages/coding-agent/src/sdk.ts)、[helpers.ts](https://github.com/can1357/oh-my-pi/blob/v18.4.4/packages/coding-agent/src/discovery/helpers.ts)。源码副本存于 reports；这属于源码分析，不能替代对二进制全部文件读取行为的跟踪。

现已补上两层防护：祖先目录存在这两个名称时，在新建profile和启动worker之前拒绝；生成的独立配置显式禁用 `agents-md`、`claude-md`。错误只指出已知标记名称，不读取或输出其内容。[提供器过滤实现](https://github.com/can1357/oh-my-pi/blob/v18.4.4/packages/coding-agent/src/capability/index.ts) 在发现前排除 disabledProviders；配置项来源于 [model-settings.ts](https://github.com/can1357/oh-my-pi/blob/v18.4.4/packages/coding-agent/src/config/model-settings.ts)。这不是全部配置来源的允许列表，也不是操作系统沙箱。

新增回归用例覆盖两种祖先文件、无效UTF-8内容不读取、不输出、不改写、拒绝时无profile，以及同名目录和原有 .codex/.omp/.git/.env 检查。不得因为零prompt握手成功便宣称隐含上下文或真实模型编程全部验收。

部署方案下一步必须单独解决：保留受控数据目录与运行目录的身份关联，允许选择经核查的独立运行位置；先完成创建权限、非链接路径、并发占用、重开及卸载保留策略，再接产品配置。若要直接支持任意用户目录，则需要有明确配置发现边界的受控启动方式，并用合成祖先配置验证读取、插件启动和隐含网络行为；仅增加一个 .git 或空配置目录不足以证明所有发现路径被截断。当前没有迁移用户目录、修改第三方安装或放宽隔离。

本批收尾（03:14 +08:00）：284默认Rust通过/0失败/51忽略，另显式3项实际离线进程回归通过；fmt、严格clippy、前端/核心/桌面debug构建及4Node通过。真实固定OMP握手与两项启动前拒绝通过，进程退出0/剩余0，模型prompt0；最终报告reports/omp-profile-audit-final-20261010/result.json。此轮没有重跑原生窗口流程、发布或安装。首版检查脚本正确识别两项预期拒绝，却把最后一次拒绝的exit1传到外层；已显式收口成功exit0并用新目录重跑，失败日志保留。索引reports/p3-omp-profile-review-20261010.json。
