//! Rebuildable reading view of trusted ledger events. Never an authorization input.
use chrono::{DateTime, FixedOffset, SecondsFormat};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandoffStatus {
    Running,
    Submitted,
    Blocked,
    Accepted,
    Cancelled,
}
impl HandoffStatus {
    fn text(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Submitted => "submitted",
            Self::Blocked => "blocked",
            Self::Accepted => "accepted",
            Self::Cancelled => "cancelled",
        }
    }
    fn terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Cancelled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffEvent {
    pub sequence: u64,
    pub message_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub actor: String,
    pub received_at: String,
    pub occurred_at: String,
    pub status: HandoffStatus,
    pub summary: String,
    pub next_action: String,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyVolume {
    pub path: String,
    pub day: String,
    pub number: usize,
    pub sealed: bool,
    pub event_ids: Vec<String>,
    pub markdown: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HandoffOptions {
    pub max_new_tasks: usize,
    pub max_bytes: usize,
}
impl Default for HandoffOptions {
    fn default() -> Self {
        Self {
            max_new_tasks: 30,
            max_bytes: 256 * 1024,
        }
    }
}
fn timestamp(s: &str) -> Result<DateTime<FixedOffset>, String> {
    let b = s.as_bytes();
    // Same timestamp grammar as the prototype, with strict calendar validation.
    let shape = b.len() >= 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && (0..19)
            .filter(|i| ![4, 7, 10, 13, 16].contains(i))
            .all(|i| b[i].is_ascii_digit());
    if !shape {
        return Err("invalid timestamp".into());
    }
    let mut zone = 19;
    if b[zone] == b'.' {
        zone += 1;
        let start = zone;
        while zone < b.len() && b[zone].is_ascii_digit() {
            zone += 1;
        }
        if !(1..=3).contains(&(zone - start)) {
            return Err("invalid timestamp precision".into());
        }
    }
    let z = &b[zone..];
    if z != b"Z"
        && !(z.len() == 6
            && matches!(z[0], b'+' | b'-')
            && z[3] == b':'
            && [1, 2, 4, 5].iter().all(|i| z[*i].is_ascii_digit()))
    {
        return Err("timestamp timezone required".into());
    }
    let t = DateTime::parse_from_rfc3339(s).map_err(|_| "invalid timestamp")?;
    if &s[17..19] == "60" {
        return Err("leap second unsupported".into());
    }
    Ok(t)
}
fn local(
    s: &str,
    zone: Option<cerulean_domain::timezone::Tz>,
) -> Result<(String, String, String, String), String> {
    let instant = timestamp(s)?;
    let t = match zone {
        Some(zone) => instant.with_timezone(&zone).fixed_offset(),
        None => instant.with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap()),
    };
    Ok((
        t.format("%Y-%m-%d").to_string(),
        t.format("%H%M%S").to_string(),
        t.to_rfc3339_opts(SecondsFormat::Millis, false),
        t.format("%z").to_string(),
    ))
}
fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.replace("\r\n", "\n").chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push(' '),
            c if "\\`*_{}[]()#+.!|~-".contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}
fn open(
    volumes: &mut Vec<DailyVolume>,
    event: &HandoffEvent,
    new_day: bool,
    states: &[&HandoffEvent],
    options: &HandoffOptions,
    zone: Option<cerulean_domain::timezone::Tz>,
) -> Result<(), String> {
    let (day, clock, full, offset) = local(&event.received_at, zone)?;
    let number = if new_day {
        1
    } else {
        volumes.last().unwrap().number + 1
    };
    if let Some(v) = volumes.last_mut() {
        v.sealed = true;
    }
    let mut markdown = format!("# 每日交接 {day} / {number}\n\n开始接收：{full}\n\n");
    if new_day {
        let unfinished: Vec<_> = states.iter().filter(|e| !e.status.terminal()).collect();
        if !unfinished.is_empty() {
            let mut index = "跨日未完成任务：\n\n".to_string();
            for e in &unfinished {
                index += &format!("- {}：{}\n", escape(&e.task_id), e.status.text());
            }
            index.push('\n');
            if (markdown.len() + index.len()) * 2 < options.max_bytes {
                markdown += &index;
            } else {
                markdown += &format!(
                    "跨日未完成任务：{} 个；完整列表从事件账本重建。\n\n",
                    unfinished.len()
                );
            }
        }
    }
    volumes.push(DailyVolume {
        path: format!("daily/{day}/handoff_{day}_{number:03}_{clock}{offset}.md"),
        day,
        number,
        sealed: false,
        event_ids: vec![],
        markdown,
    });
    Ok(())
}
pub fn project_handoffs(
    input: &[HandoffEvent],
    options: &HandoffOptions,
) -> Result<Vec<DailyVolume>, String> {
    project_handoffs_with_zone(input, options, None)
}

/// New scoped projects use named IANA rules. Legacy schema 1 uses the function above.
pub fn project_handoffs_in_timezone(
    input: &[HandoffEvent],
    options: &HandoffOptions,
    timezone: &str,
) -> Result<Vec<DailyVolume>, String> {
    cerulean_domain::timezone::require_archived_iana_rules()?;
    project_handoffs_with_zone(
        input,
        options,
        Some(cerulean_domain::timezone::parse(timezone)?),
    )
}

fn project_handoffs_with_zone(
    input: &[HandoffEvent],
    options: &HandoffOptions,
    zone: Option<cerulean_domain::timezone::Tz>,
) -> Result<Vec<DailyVolume>, String> {
    project_handoff_refs(input.iter(), options, zone)
}

pub(crate) fn project_handoff_refs<'a>(
    input: impl Iterator<Item = &'a HandoffEvent>,
    options: &HandoffOptions,
    zone: Option<cerulean_domain::timezone::Tz>,
) -> Result<Vec<DailyVolume>, String> {
    if options.max_new_tasks == 0 || options.max_bytes < 512 {
        return Err("invalid limits".into());
    }
    let mut unique = Vec::new();
    let mut seen = BTreeMap::new();
    for event in input {
        if event.sequence == 0 || event.sequence > 9_007_199_254_740_991 {
            return Err("invalid sequence".into());
        }
        for id in [
            &event.message_id,
            &event.task_id,
            &event.attempt_id,
            &event.actor,
        ] {
            if id.is_empty()
                || id.len() > 80
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return Err("invalid identifier".into());
            }
        }
        timestamp(&event.received_at)?;
        timestamp(&event.occurred_at)?;
        if let Some(prev) = seen.insert(&event.message_id, event) {
            if prev != event {
                return Err("conflicting duplicate message".into());
            }
        } else {
            unique.push(event);
        }
    }
    unique.sort_by_key(|e| e.sequence);
    for (i, e) in unique.iter().enumerate() {
        if e.sequence != i as u64 + 1 {
            return Err("ledger sequence gap or collision".into());
        }
        if i > 0 && timestamp(&e.received_at)? < timestamp(&unique[i - 1].received_at)? {
            return Err("receipt clock moved backwards".into());
        }
    }
    let mut volumes: Vec<DailyVolume> = Vec::new();
    // Preserve original task insertion order for the cross-day index.
    let mut states: Vec<&HandoffEvent> = Vec::new();
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut new_count = 0;
    for e in unique {
        let (day, _, full, _) = local(&e.received_at, zone)?;
        if volumes.last().is_none_or(|v| v.day != day) {
            open(&mut volumes, e, true, &states, options, zone)?;
            new_count = 0;
        }
        let position = states.iter().position(|old| old.task_id == e.task_id);
        let mut entry = format!(
            "## {} · {}\n\n消息：{}；执行者：{}；尝试：{}\n\n接收：{}；发生：{}；状态：{}\n\n",
            e.sequence,
            escape(&e.task_id),
            escape(&e.message_id),
            escape(&e.actor),
            escape(&e.attempt_id),
            full,
            escape(&e.occurred_at),
            e.status.text()
        );
        if local(&e.occurred_at, zone)?.0 != day {
            entry += "跨日到达，按接收日归档。\n\n";
        }
        let evidence = e
            .evidence
            .iter()
            .map(|s| escape(s))
            .collect::<Vec<_>>()
            .join("；");
        entry += &format!(
            "内容：{}\n\n证据：{}\n\n下一步：{}\n\n",
            escape(&e.summary),
            if evidence.is_empty() {
                "无"
            } else {
                &evidence
            },
            escape(&e.next_action)
        );
        if let Some(path) = first.get(&e.task_id) {
            entry += &format!("初始卷：{}\n\n", escape(path));
        }
        let v = volumes.last().unwrap();
        if !v.event_ids.is_empty()
            && ((position.is_none() && new_count >= options.max_new_tasks)
                || v.markdown.len() + entry.len() > options.max_bytes)
        {
            open(&mut volumes, e, false, &states, options, zone)?;
            new_count = 0;
        }
        let v = volumes.last_mut().unwrap();
        if v.markdown.len() + entry.len() > options.max_bytes {
            return Err("event exceeds volume budget; store large details as artifacts".into());
        }
        v.markdown += &entry;
        v.event_ids.push(e.message_id.clone());
        if let Some(i) = position {
            states[i] = e;
        } else {
            new_count += 1;
            first.insert(e.task_id.clone(), v.path.clone());
            states.push(e);
        }
    }
    Ok(volumes)
}
