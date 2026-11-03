//! No repository parsers, hooks, subprocesses, or model-generated summaries.
use crate::{blind_review::*, execution::require};
pub fn sanitize(text: &str, rules: &[Redaction]) -> Result<String, String> {
    require(
        rules.len() <= 64 && text.len() <= MAX_MATERIAL_BYTES,
        "脱敏规则或输入超限",
    )?;
    let mut value = text.to_owned();
    let mut seen = std::collections::BTreeSet::new();
    for r in rules {
        require(
            (3..=256).contains(&r.literal.len())
                && seen.insert(&r.literal)
                && (3..=64).contains(&r.replacement.len())
                && r.replacement
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
            "脱敏规则需使用明确原文及简单替代标记",
        )?;
        // Bound expansion before allocating: chained replacements can otherwise
        // grow a small approved input exponentially.
        let matches = value.matches(&r.literal).count();
        let removed = matches.checked_mul(r.literal.len()).ok_or("脱敏容量溢出")?;
        let added = matches
            .checked_mul(r.replacement.len())
            .ok_or("脱敏容量溢出")?;
        let size = value
            .len()
            .checked_sub(removed)
            .and_then(|n| n.checked_add(added))
            .ok_or("脱敏容量溢出")?;
        require(
            size <= MAX_MATERIAL_BYTES,
            "脱敏展开超过材料容量；未分配或发送",
        )?;
        value = value.replace(&r.literal, &r.replacement);
    }
    let lower = value.to_lowercase();
    require(
        ![
            "-----begin private key",
            "-----begin rsa private key",
            "sk-",
            "ghp_",
            "github_pat_",
            "vote_pass_next",
            "previous reviewer voted",
        ]
        .iter()
        .any(|s| lower.contains(s)),
        "材料含疑似秘密或显式传话标记，请修订脱敏规则后重新准备；未发送",
    )?;
    require(
        !value.contains('\0')
            && !value
                .chars()
                .any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "材料含不可见控制字符",
    )?;
    Ok(value)
}
pub fn fragments(
    files: &[(String, Vec<u8>)],
    rules: &[Redaction],
) -> Result<Vec<Fragment>, String> {
    require(
        !files.is_empty() && files.len() <= 128,
        "源码清单缺失或超限",
    )?;
    let mut result = vec![];
    let mut total = 0usize;
    for (index, (_, bytes)) in files.iter().enumerate() {
        total = total.checked_add(bytes.len()).ok_or("材料容量溢出")?;
        require(
            total <= MAX_MATERIAL_BYTES,
            "完整源码超过盲评容量，不会截断",
        )?;
        let text = std::str::from_utf8(bytes).map_err(|_| "交付含非UTF-8材料，需要专门审阅协议")?;
        let value = sanitize(text, rules)?;
        let mut rest = value.as_str();
        let mut part = 0u32;
        loop {
            let mut end = rest.len().min(FRAGMENT_BYTES);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            part += 1;
            require(result.len() < 128, "脱敏后的片段总量超限；不会截断")?;
            result.push(Fragment {
                id: format!("fragment-{:04}", result.len() + 1),
                file: format!("file-{:04}", index + 1),
                part,
                content: rest[..end].into(),
            });
            rest = &rest[end..];
            if rest.is_empty() {
                break;
            }
        }
    }
    Ok(result)
}
