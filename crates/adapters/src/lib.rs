use cerulean_domain::execution::{AdapterCapability, Capability, CapabilityState};
use cerulean_domain::AdapterInfo;
pub mod api_generation;
pub mod codex;
pub mod codex_observation;
pub mod deputy_management;
pub mod omp_protocol;
pub mod omp_transport;
pub mod openai;
mod presentation;
pub mod provider_models;
pub use presentation::display_catalog;
/// Registration describes intended scope, never an authenticated or usable connection.
/// This legacy catalog is part of persisted management request digests. Keep its
/// serialized values stable; user-facing descriptions belong in display_catalog.
pub fn catalog() -> Vec<AdapterInfo> {
    current_catalog()
        .into_iter()
        .filter(|a| a.id != "a6api")
        .collect()
}
/// Current registrations for new configuration/context and presentation. The
/// reader16 relay extension must not overwrite historical request catalogs.
pub fn current_catalog() -> Vec<AdapterInfo> {
    [
        (
            "codex",
            "Codex",
            "Agent / app-server",
            "已接入受控编程与持久记录；真实主副指挥完整链仍待验收",
        ),
        (
            "omp",
            "OMP",
            "Agent / RPC",
            "已接入后台调度与文件工具；真实模型编程仍待验收",
        ),
        (
            "qoder",
            "Qoder",
            "现有桌面会话",
            "已接窗口发现与用户会话关联；原型 GLM 配置 20/20，产品发送仍待接入",
        ),
        (
            "workbuddy",
            "WorkBuddy",
            "现有桌面会话",
            "已接窗口发现与用户会话关联；原型 20/20，内容指纹不等于供应商会话ID",
        ),
        (
            "openai",
            "OpenAI / GPT",
            "API / 工作台 CLI",
            "主副指挥限定 OpenAI；已接官方 API 与独立凭证，真实链路待验收",
        ),
        (
            "anthropic",
            "Claude",
            "API / 工作台 CLI",
            "已接 API 凭证、文件工具与持续调度；真实供应商链和 CLI 待验收",
        ),
        (
            "deepseek",
            "DeepSeek",
            "API / 工作台 CLI",
            "已接 API 与 OMP 编程调度；真实供应商链和 CLI 待验收",
        ),
        (
            "qwen",
            "Qwen",
            "API / 工作台 CLI",
            "已接分地域 API、文件工具与持续调度；真实供应商链和 CLI 待验收",
        ),
        (
            "kimi",
            "Kimi",
            "API / 工作台 CLI",
            "已接中国站 API、文件工具与持续调度；真实供应商链和 CLI 待验收",
        ),
        (
            "a6api",
            "A6API 中转",
            "Chat Completions API / 工作台 CLI",
            "独立凭证与固定中转地址；指定配置的真实编程链待验收",
        ),
        (
            "grok",
            "xAI Grok",
            "API",
            "已接官方 API、文件工具与持续调度；真实供应商链待验收",
        ),
    ]
    .into_iter()
    .map(|(id, name, transport, note)| AdapterInfo {
        id: id.into(),
        name: name.into(),
        transport: transport.into(),
        status: if matches!(id, "qoder" | "workbuddy") {
            "prototype"
        } else {
            "partial"
        }
        .into(),
        note: note.into(),
        capabilities: [
            Capability::Discover,
            Capability::Bind,
            Capability::Send,
            Capability::Observe,
            Capability::Reconcile,
            Capability::Cancel,
            Capability::Resume,
            Capability::Usage,
            Capability::Approval,
            Capability::Tools,
        ]
        .into_iter()
        .map(|capability| AdapterCapability {
            capability,
            state: CapabilityState::Unverified,
            tested_version: None,
            evidence_sha256: None,
        })
        .collect(),
    })
    .collect()
}
