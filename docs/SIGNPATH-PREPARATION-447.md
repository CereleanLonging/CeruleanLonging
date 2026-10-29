# CeruleanLonging 的 SignPath 免费开源签名准备方案

2026-10-27。用户指定项目维护者名称 `lanlu`，选择先准备免费开源 SignPath 方案，暂不公开。本文是本地准备稿，不是已申请、已签名或已获发布许可的证明。

## 已确认的边界

当前仓库没有配置 Git 远程，根目录没有项目 LICENSE；现有 `.github/workflows/checks.yml` 只做构建检查。第三方许可材料已存在，但不等于本项目已取得开源授权。不能把整个工作目录作为公开仓库上传：`reports` 中包含受保护凭证、测试现场和历史记录。

用户指定的 `lanlu` 用作维护者/品牌名称。SignPath Foundation 免费证书的发布者由基金会提供，不能承诺 Windows 的证书发布者显示为 `lanlu`。GitHub Actions 是构建及提交渠道，不自行签发 Authenticode 证书。

官方方案要求项目满足开源、维护和发行条件，接受申请审核。通过 GitHub 的开源签名流程需要 GitHub 托管 runner 的可核实构建来源，不能将本机旧 EXE 上传后冒称为该流程构建。来源：[基金会条件](https://signpath.org/terms)、[申请入口](https://signpath.org/apply)、[GitHub 集成](https://docs.signpath.io/trusted-build-systems/github)。

## 申请材料草稿

| 字段 | 准备值或待办 |
| --- | --- |
| 项目名称 | CeruleanLonging |
| 维护者/品牌 | lanlu；联系人及邮箱待用户提供 |
| 产品说明 | Windows 本地多模型编程工作台；独立核心管理批准、任务、恢复和验收，桌面界面提供操作入口 |
| 代码仓库和下载页 | 尚未配置；此次不创建或公开 |
| 开源许可证 | 待用户确定，例如在核完第三方和贡献来源后评估 MIT 或 Apache-2.0；不替用户授予许可 |
| 当前平台 | Windows x64；已有 Windows 10 19045.3930 的有限范围实测 |
| 当前发行状态 | 0.1.26 内部验证候选，未签名，完整正式验收未完成 |
| 待签产物 | GUI `CeruleanLonging.exe`、核心 `cerulean-runtime.exe`、NSIS `setup.exe` |
| 构建来源 | 准备新的经审阅提交及 GitHub-hosted Windows 构建；当前本地文件不冒充该来源 |
| 签名人员 | 提交人、审核人及 SignPath policy 待组织配置，不能虚构现有团队 |

## 本项目需要的构建和签名顺序

1. 准备独立、可审阅的公开源码候选清单。包含产品源码、锁文件、构建脚本、必要测试夹具和合法分发的依赖材料；排除凭证、用户数据、聊天、VM、运行日志和本地绝对路径配置。当前仍只准备清单，公开前再由用户批准。
2. 在明确的提交上用 GitHub 托管 Windows runner 构建。先处理当前依赖检查中本机路径绑定与 CI 路径的差异；保留静态 CRT、许可证和构建输入核验，不通过关闭检查解决移植问题。固定工具链、依赖和 Actions 的审核版本。
3. 将自有 GUI/runtime 作为第一阶段签名对象。上传限定文件集的 workflow artifact，提交 SignPath，取得签名回执并核实产物、签名链、用途和时间戳。签名前后分别记录摘要，因为签名会改变文件字节。
4. 使用已签的 GUI/runtime 打包 NSIS，再对安装器进行第二阶段签名。**不能直接重跑当前 `scripts/desktop.mjs package` 来保留第一阶段签名**：该脚本会重建并重新复制 runtime。需先实现并验证只打包已核验产物的入口，或经验证可保留签名的工具集成；未完成前不启用签名 workflow。
5. 对最终安装器及实际内嵌 GUI/runtime 重新验证。复用 `scripts/review_nsis_payload.py` 和现有包审阅机制，核对版本、签名、时间戳、资源、文件摘要及安装后落盘字节。第三方资源保留其归属，不改成自有软件签名声明。
6. 生成新签名候选的 manifest 和验收记录，保留旧 0.1.26 冻结包。不得只把旧 manifest 的 `NotSigned` 改成 `Valid`，也不得将 GitHub 的源码签名或 artifact attestation 当作 EXE 的 Authenticode 签名。

## 后续集成参数

组织 ID、project slug、artifact configuration slug、signing policy slug 和 `SIGNPATH_API_TOKEN` 由 SignPath 项目创建后提供。令牌只进入 GitHub 的受保护 Secret；第一阶段和安装器阶段使用独立、明确的文件集与 artifact ID。采用手动发行入口和受保护环境，签名后不自动发布 Release。

官方集成使用 `actions/upload-artifact` 返回的 artifact ID，再调用 `signpath/github-action-submit-signing-request`；实际启用时固定经过审核的提交 SHA，并根据实际使用版本核对归档格式。此次没有往 `.github/workflows` 放入可运行签名任务，也没有配置 Secret、创建仓库、提交申请、上传二进制或改变本机信任。

## 完成判据与当前阻塞

签名门槛只有在申请获批、真实产物签名验证通过、最终安装载荷一致且来源可核查后才能关闭。许可证选择、联系人、公开仓库、基金会审核、CI 构建移植和两阶段打包仍待完成。健康数据升级/回退/卸载保留数据及供应商、组合和效率测试仍是独立门槛；签名成功也不会自动关闭它们。
