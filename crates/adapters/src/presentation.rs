//! Product descriptions are independent of persisted management request bytes.
use cerulean_domain::AdapterInfo;

/// Describes implemented scope and bounded verification, never current login,
/// permission to dispatch, or blanket capability certification.
pub fn display_catalog() -> Vec<AdapterInfo> {
    let mut entries = super::supported_catalog();
    for entry in &mut entries {
        let note = match entry.id.as_str() {
            "codex" => "支持编程执行；当前 Codex 会话可担任主指挥，执行仍需方案批准",
            "omp" => "所测配置已完成真实编程、原任务建议及验收；其他版本和模型仍需核验",
            "qoder" => "所测 Qwen-3.8-Flash 配置已完成产品编程验收和自动发送；完整故障矩阵及严格原任务监督仍待验收",
            "openai" => {
                "支持 GPT 执行者和 API 管理来源；官方直连实测延期，兼容中转的证据按对应供应商分别记录"
            }
            "a6api" => "所测 gpt-6.1-sol 已完成真实规划、分配、编程、独立验收及两次重开；其他模型与完整故障矩阵仍待核验",
            "deepseek" => "支持 API 编程与副指挥；所测配置的真实三级编程闭环已通过",
            _ => continue,
        };
        entry.note = note.into();
    }
    entries
}

#[cfg(test)]
mod tests {
    #[test]
    fn presentation_preserves_historical_management_catalog_and_capability_gates() {
        // Captured from the actual 218 Plan handoff, before display descriptions
        // were separated. Updating this fixture requires a protocol migration.
        let historic: serde_json::Value =
            serde_json::from_str(include_str!("catalog_legacy_fixture.json")).unwrap();
        let legacy = super::super::catalog();
        assert_eq!(serde_json::to_value(&legacy).unwrap(), historic);
        let display = super::display_catalog();
        let current = super::super::current_catalog();
        assert_eq!(current.len(), legacy.len() + 1);
        assert_eq!(current.iter().filter(|a| a.id == "a6api").count(), 1);
        assert_eq!(display.len(), current.len() - 1);
        assert!(!display.iter().any(|a| a.id == "workbuddy"));
        for (shown, original) in display
            .iter()
            .zip(current.into_iter().filter(|a| a.id != "workbuddy"))
        {
            assert_eq!(shown.id, original.id);
            assert_eq!(shown.name, original.name);
            assert_eq!(shown.transport, original.transport);
            assert_eq!(shown.capabilities, original.capabilities);
        }
        assert!(!display.iter().any(|a| a.status == "verified"));
    }
}
