# Code signing policy / 代码签名政策

[简体中文](#zh) · [English](#en)

<a id="zh"></a>

## 简体中文

CeruleanLonging 的维护者和产品发布者为 **lantu**（GitHub：[CereleanLonging](https://github.com/CereleanLonging)）。0.1.34 公开预览安装包目前**未签名**。项目正在准备向 SignPath Foundation 申请免费开源签名；本页不表示已经获批或取得证书。

- 代码作者、提交维护者和外部贡献审阅者：lantu。
- 签名审批负责人：lantu；每次签名须由负责人审阅具体提交、构建来源、产物和权限后批准。
- 维护者已确认 GitHub 开启双重验证；SignPath 账户建立后也须启用，启用之前不执行签名。
- 仅为本项目自己的 Windows GUI、Rust 核心和安装器申请签名。第三方组件保留原许可证与签名，不冒用本项目签名重新签署上游程序。
- 拟采用可追溯到固定公开提交的 GitHub 托管 Windows 构建。先核对并签署自有内层程序，再保持内层字节封装与签署外层安装器，最后核对安装和运行结果。

若申请获批，免费签名服务由 [SignPath.io](https://signpath.io) 提供，证书主体为 [SignPath Foundation](https://signpath.org)，与产品中的 lantu 署名不同。实际获批、签名和验证状态将在发布页更新；不会把哈希值或未签名文件当成可信签名。

用户数据和外部服务使用见 [隐私说明](PRIVACY.md)。完整规则以 [SignPath Foundation 条款](https://signpath.org/terms) 为准。

<a id="en"></a>

## English

CeruleanLonging is maintained and published by **lantu** ([CereleanLonging on GitHub](https://github.com/CereleanLonging)). The 0.1.34 public-preview installer is currently **unsigned**. The project is preparing an application for free open-source signing through SignPath Foundation. This page does not claim approval or an issued certificate.

- Author, committer, and reviewer of external contributions: lantu.
- Signing approver: lantu. Each signing request requires review of its exact commit, build origin, artifacts, and permissions.
- The maintainer has confirmed GitHub MFA is enabled. MFA must also be enabled on the SignPath account before signing.
- The proposed signing scope covers the project's own Windows GUI, Rust runtime, and installer. Third-party components retain their licenses and existing signatures; upstream executables are not re-signed as project-owned code.
- The planned workflow uses GitHub-hosted Windows builds traceable to a fixed public commit. Project-owned inner executables are verified and signed before byte-preserving packaging; the installer is then signed, verified, and tested.

If approved, free code signing would be provided by [SignPath.io](https://signpath.io), with the certificate issued to [SignPath Foundation](https://signpath.org). That certificate identity differs from the lantu product label. Actual approval, signing, and verification status will be reported on release pages. Checksums and unsigned binaries are never represented as trusted signatures.

See the [Privacy notice](PRIVACY.md) for data and external-service use, and the [SignPath Foundation terms](https://signpath.org/terms) for program requirements.
