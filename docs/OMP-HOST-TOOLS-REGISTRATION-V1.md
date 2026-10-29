# OMP 宿主工具注册

2026-10-10。工作台已可向OMP18.4.4注册固定定义的宿主工具，并核对回执和实际工具目录。注册尚不提供工具处理器；现有无工具执行入口在工具已注册时拒绝派发。后续需接受控文件/命令服务与独立调用记录，才能启用真实编程。

## 协议与入口

依据固定tag官方 [rpc-types.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/coding-agent/src/modes/rpc/rpc-types.ts)、[host-tools.ts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.4.4/packages/coding-agent/src/modes/rpc/host-tools.ts) 和既有rpc-mode源码；本地副本位于reports。官方set_host_tools返回toolNames，工具执行另行通过host_tool_call和host_tool_result交接。

新增 `worker::host_tools::HostToolDefinition` 和 `WorkerTransport::register_host_tools`。注册只接受1–8个唯一cerulean_名称、有界描述与闭合object参数格式；在任何新RPC前验证定义。必须是尚未执行、已握手的独占连接，每个连接只注册一次。

先get_state检查原session/provider/model空闲且工具目录为空，再发送一次set_host_tools。ACK必须恰好包含全部唯一名称，随后新的get_state还须保持原空闲会话，工具数、名称、描述和参数JSON全部等于宿主定义。额外原生工具、不同格式或模型/会话变化均拒绝。注册future被丢弃或RPC错误后连接不可复用，不重试注册和prompt。

## 当前证据

真实受控OMP探测 `reports/omp-host-registration-20261010-005517.json` 通过：negotiate_protocol1次、get_state3次、set_host_tools1次；注册cerulean_read_file，目录与定义一致。prompt和工具调用均0，使用明确无效占位凭证，未读真实凭证。PID25768，自然退出码0、剩余进程0、stderr0；独立目录 `D:\test\omp-host-registration-20261010-005517` 保留。只证明实际注册兼容，未读取项目文件。

新增5组协议测试覆盖回执/实时目录匹配、原生工具拒绝、错误或重复名称/格式/身份、连接已建立时的无效定义不产生新RPC，以及注册已送达而未来被丢弃后不重发。原无工具执行仍不能因注册存在而获得工具权限。适配器54项通过，最终工作区回归和严格clippy结果记录于证据索引。

初版测试共享辅助函数的可见性不足，改为测试模块内部pub(super)后通过；中断测试临时identity借用生命周期编译失败，改为局部绑定后通过，两份失败日志保留。生产权限和协议约束未放宽。

## 下一步

实现唯一RPC读取者下的工具调用/取消/结果路由、有限并发队列和持久调用意图，严格绑定当前批准task/attempt/request与工具定义。宿主服务仅允许批准范围内读取、受控文件变更和明确批准的命令；拒绝路径逃逸、链接、凭证访问和未审命令。实际动作结果或错误须留证据，未知不自动重做有副作用的调用。然后将凭证、进程与工作流接入RuntimeHost及原生界面，再进行真实编程/取消/恢复。

OMP能力仍未验收，完整P3/P6未通过。注册批次未重跑前端/Node/原生界面或发布安装；前一凭证批次的证据另存。全部首批API/CLI、真实OpenAI指挥链、Qoder/WorkBuddy已有会话、并行集成及效率范围保持。

收尾：241项常规Rust通过、41项忽略；适配器54项、fmt及all-targets/custom-protocol严格clippy通过。既有验收入账间歇失败本次未复现，根因仍待查。全部本批测试/构建/探测句柄结束。证据索引reports/p3-omp-host-registration-review-20261010.json；所列文件副本reports/p3-omp-host-registration-sources-20261010，非完整构建归档。

后续进展（2026-10-10）：[调用路由与持久记录](OMP-HOST-CALLS-V1.md) 已增加显式工具服务入口；原无工具入口继续拒绝非空目录。实际生产文件/命令服务及调度尚待接入。
