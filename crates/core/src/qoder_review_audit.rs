//! Read-only audit of the bound Qoder session's versioned message envelopes.
use cerulean_domain::{
    blind_review::*,
    execution::{digest, require},
};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub(crate) struct Audit {
    database: PathBuf,
    session: String,
    cwd: String,
    model: String,
    prefix: Vec<(i64, String, String)>,
}
fn open(path: &Path) -> Result<Connection, String> {
    crate::host::ordinary(path, false)?;
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "Qoder评分记录无法只读打开")?;
    db.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "Qoder记录读取超时设置失败")?;
    db.pragma_update(None, "query_only", true)
        .map_err(|_| "Qoder记录只读状态未确认")?;
    Ok(db)
}
fn metadata(db: &Connection, session: &str) -> Result<(String, String), String> {
    db.query_row(
        "SELECT CASE WHEN length(cwd)<=4096 THEN cwd ELSE NULL END,CASE WHEN length(model)<=200 THEN model ELSE NULL END FROM chat_sessions WHERE session_id=?1 AND deleted_at IS NULL",
        [session],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(|_| "Qoder原会话元数据缺失".into())
}
fn prefix(db: &Connection, session: &str) -> Result<Vec<(i64, String, String)>, String> {
    let mut stmt = db.prepare("SELECT sequence,CASE WHEN length(message_id)<=200 THEN message_id ELSE NULL END,CASE WHEN length(payload_hash)=64 THEN payload_hash ELSE NULL END FROM chat_session_messages WHERE session_id=?1 ORDER BY sequence LIMIT 1025").map_err(|_|"Qoder评分记录结构不支持")?;
    let values = stmt
        .query_map([session], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|_| "Qoder会话索引不可读取")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Qoder会话索引无效")?;
    require(values.len() <= 1024, "Qoder评分记录超限")?;
    require(
        values.windows(2).all(|p| p[0].0 < p[1].0),
        "Qoder记录顺序无效",
    )?;
    Ok(values)
}
impl Audit {
    pub(crate) fn begin(workspace: &Path, session: &str) -> Result<Self, String> {
        let paths = [
            workspace
                .join(".cerulean-runtime/home/AppData/Roaming/com.qoder.app.stable/main.sqlite"),
            workspace.join(".cerulean-runtime/appdata/com.qoder.app.stable/main.sqlite"),
            workspace.join(".cerulean-runtime/desktop/main.sqlite"),
            workspace.join(".cerulean-runtime/desktop/desktop-data/main.sqlite"),
            workspace.join(".cerulean-runtime/appdata/Qoder/main.sqlite"),
        ];
        let mut matched = vec![];
        for path in paths.into_iter().filter(|p| p.exists()) {
            crate::artifacts::root_path(
                path.parent()
                    .ok_or("Qoder记录目录缺失")?
                    .to_str()
                    .ok_or("Qoder记录目录无效")?,
            )?;
            let db = open(&path)?;
            if let Ok((cwd, model)) = metadata(&db, session) {
                require(
                    crate::artifacts::root_path(&cwd)?
                        == crate::artifacts::root_path(
                            workspace.to_str().ok_or("Qoder工作目录无效")?,
                        )?,
                    "Qoder原会话目录不符",
                )?;
                let head = prefix(&db, session)?;
                matched.push(Self {
                    database: path,
                    session: session.into(),
                    cwd,
                    model,
                    prefix: head,
                });
            }
        }
        require(matched.len() == 1, "Qoder完整工具调用记录缺失或存在歧义")?;
        Ok(matched.remove(0))
    }
    pub(crate) fn finish(
        self,
        request: &str,
        prompt: &str,
        reply: &str,
        marker: &str,
    ) -> Result<(String, ReviewToolAudit), String> {
        let db = open(&self.database)?;
        let transaction = db
            .unchecked_transaction()
            .map_err(|_| "Qoder记录快照不可用")?;
        require(
            metadata(&transaction, &self.session)? == (self.cwd, self.model),
            "Qoder评分期间模型或目录改变",
        )?;
        let current = prefix(&transaction, &self.session)?;
        require(
            current.starts_with(&self.prefix),
            "Qoder原历史在评分期间变化",
        )?;
        let after = self.prefix.last().map_or(-1, |p| p.0);
        let mut statement = transaction.prepare("SELECT CASE WHEN length(role)<=32 THEN role ELSE NULL END,CASE WHEN length(status)<=32 THEN status ELSE NULL END,CASE WHEN length(CAST(payload_json AS BLOB))<=524288 THEN payload_json ELSE NULL END,CASE WHEN length(payload_hash)=64 THEN payload_hash ELSE NULL END,payload_bytes,payload_schema_version,CASE WHEN length(turn_id)<=200 THEN turn_id ELSE NULL END FROM chat_session_messages WHERE session_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 65").map_err(|_|"Qoder评分事件结构不支持")?;
        let rows = statement
            .query_map(rusqlite::params![self.session, after], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|_| "Qoder评分事件不可读取")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Qoder评分事件不完整")?;
        audit_rows(request, prompt, reply, marker, &rows)
    }
}
type Row = (String, String, String, String, i64, i64, Option<String>);

// Qoder 0.4.8 keeps role and turn identity in SQLite columns and represents
// every event in parts. Validate that version's exact envelope before sharing
// the existing text/structural-tool audit with version 1.
fn version_two_message(
    role: &str,
    turn: Option<&str>,
    message: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let turn = turn.filter(|s| !s.is_empty()).ok_or("Qoder评分轮次缺失")?;
    let fields = message.as_object().ok_or("Qoder评分JSON无效")?;
    let parts = message["parts"].as_array().ok_or("Qoder评分过程片段缺失")?;
    let mut ids = std::collections::HashSet::new();
    let mut tools = vec![];
    for part in parts {
        let id = part["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Qoder事件标识缺失")?;
        require(ids.insert(id), "Qoder事件标识重复")?;
        if !part["parentToolUseId"].is_null() {
            tools.push(serde_json::json!({"name":"qoder_nested_tool_event"}));
        }
        match part["type"].as_str() {
            Some("text" | "thinking") => {
                require(
                    part["text"].is_string()
                        && part.as_object().is_some_and(|fields| {
                            fields.keys().all(|key| {
                                matches!(
                                    key.as_str(),
                                    "id" | "type"
                                        | "text"
                                        | "status"
                                        | "startedAt"
                                        | "completedAt"
                                        | "durationMs"
                                        | "parentToolUseId"
                                )
                            })
                        }),
                    "Qoder文本事件不完整或存在未识别字段",
                )?;
            }
            Some("hook") => {
                require(
                    part.as_object().is_some_and(|fields| {
                        fields
                            .keys()
                            .all(|key| matches!(key.as_str(), "id" | "type" | "hook"))
                    }) && part["hook"].as_object().is_some_and(|fields| {
                        fields.keys().all(|key| {
                            matches!(
                                key.as_str(),
                                "id" | "event"
                                    | "status"
                                    | "startedAt"
                                    | "completedAt"
                                    | "durationMs"
                            )
                        })
                    }) && matches!(
                        part["hook"]["status"].as_str(),
                        Some("succeeded" | "failed")
                    ),
                    "Qoder自动化事件包含未核验内容",
                )?;
            }
            Some("tool") => {}
            _ => return Err("Qoder评分包含未知事件类型".into()),
        }
    }
    let text = match role {
        "user" => {
            let empty_fields = [
                "attachments",
                "selectedSkillNames",
                "selectedPluginIds",
                "selectedConnectorIds",
                "selectedAgentNames",
                "selectedCapabilityCommands",
                "referencedChatSessions",
            ];
            require(
                fields.keys().all(|key| {
                    empty_fields.contains(&key.as_str())
                        || matches!(key.as_str(), "issueId" | "parts")
                }) && message["issueId"].is_null()
                    && empty_fields
                        .iter()
                        .all(|key| message[key].as_array().is_some_and(Vec::is_empty))
                    && parts.len() == 1
                    && parts[0]["type"] == "text"
                    && parts[0]["id"] == format!("{turn}:text"),
                "Qoder评分输入包含附加材料或未核验能力",
            )?;
            parts[0]["text"].as_str().ok_or("Qoder评分输入缺失")?
        }
        "assistant" => {
            require(
                fields.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "forkBoundaryProviderId"
                            | "requestSetId"
                            | "resultUuid"
                            | "completedAt"
                            | "durationMs"
                            | "turnMetrics"
                            | "parts"
                            | "finalTextId"
                    )
                }) && message["requestSetId"] == turn
                    && message["completedAt"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty()),
                "Qoder评分回复轮次或封装不符",
            )?;
            let final_id = message["finalTextId"]
                .as_str()
                .ok_or("Qoder最终回复标识缺失")?;
            parts
                .iter()
                .find(|part| part["id"] == final_id && part["type"] == "text")
                .and_then(|part| part["text"].as_str())
                .ok_or("Qoder最终回复缺失")?
        }
        _ => return Err("Qoder评分存在未识别的消息角色".into()),
    };
    Ok(serde_json::json!({"role":role,"turnId":turn,"text":text,"parts":parts,"tools":tools}))
}

fn audit_rows(
    request: &str,
    prompt: &str,
    reply: &str,
    marker: &str,
    rows: &[Row],
) -> Result<(String, ReviewToolAudit), String> {
    use sha2::Digest;
    require((2..=64).contains(&rows.len()), "Qoder评分消息缺失或超限")?;
    require(
        rows.iter().all(|row| row.5 == rows[0].5)
            && (rows[0].5 == 1 || (rows[0].5 == 2 && rows.len() == 2)),
        "Qoder评分封装版本不支持或混合",
    )?;
    let mut turn = None;
    let mut user_seen = false;
    let mut last_text = None;
    let mut tools = vec![];
    let mut total = 0usize;
    for (role, status, raw, hash, bytes, schema, stored_turn) in rows {
        total = total.checked_add(raw.len()).ok_or("Qoder评分数据溢出")?;
        require(
            total <= 512 * 1024
                && *bytes == raw.len() as i64
                && format!("{:x}", sha2::Sha256::digest(raw.as_bytes())) == *hash
                && status == "completed",
            "Qoder评分封装不完整或摘要不符",
        )?;
        let mut message: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| "Qoder评分JSON无效")?;
        if *schema == 2 {
            message = version_two_message(role, stored_turn.as_deref(), message)?;
        }
        require(message["role"].as_str() == Some(role), "Qoder评分角色不符")?;
        let message_turn = message["turnId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Qoder评分轮次缺失")?;
        require(
            stored_turn.as_deref().is_none_or(|t| t == message_turn),
            "Qoder评分轮次索引不符",
        )?;
        require(
            turn.as_deref().is_none_or(|t| t == message_turn),
            "Qoder评分混入其他轮次",
        )?;
        turn = Some(message_turn.to_owned());
        let calls = message["tools"].as_array().ok_or("Qoder工具调用清单缺失")?;
        for call in calls {
            tools.push(
                call["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .ok_or("Qoder工具调用名称缺失")?
                    .to_owned(),
            );
        }
        match role.as_str() {
            "user" => {
                require(
                    !user_seen && last_text.is_none() && message["text"] == prompt,
                    "Qoder评分输入与本次分片不符",
                )?;
                user_seen = true;
            }
            "assistant" => {
                require(user_seen, "Qoder评分缺少原输入")?;
                let parts = message["parts"].as_array().ok_or("Qoder评分过程片段缺失")?;
                for part in parts {
                    match part["type"].as_str() {
                        Some("text" | "thinking") => {}
                        Some("tool") => tools.push("qoder_tool_part".into()),
                        Some("hook") => match part["hook"]["event"].as_str() {
                            Some("PreToolUse" | "PostToolUse") => {
                                tools.push("qoder_tool_hook".into())
                            }
                            Some("SessionStart" | "UserPromptSubmit" | "Stop") => {}
                            _ => return Err("Qoder评分包含无法核验的自动化事件".into()),
                        },
                        _ => return Err("Qoder评分包含未知事件类型".into()),
                    }
                }
                last_text = Some(
                    message["text"]
                        .as_str()
                        .ok_or("Qoder评分回复缺失")?
                        .to_owned(),
                );
            }
            _ => return Err("Qoder评分存在未识别的消息角色".into()),
        }
        require(tools.len() <= 32, "Qoder评分工具调用过多")?;
    }
    let original = last_text
        .as_deref()
        .and_then(|text| text.trim().strip_suffix(marker))
        .map(str::trim)
        .ok_or("Qoder评分原文缺少本轮结束标记")?;
    require(
        visible_review_matches(original, reply),
        "Qoder评分记录与原窗口回复不符",
    )?;
    let evidence_digest = digest(&(request, rows.iter().map(|r| &r.3).collect::<Vec<_>>()))?;
    Ok((
        original.to_owned(),
        ReviewToolAudit {
            status: if tools.is_empty() {
                ReviewToolStatus::Clean
            } else {
                ReviewToolStatus::ToolCallObserved
            },
            source: "desktop-history".into(),
            request: request.into(),
            evidence_digest,
            tools,
            reason: "已核对原会话的完整评分消息封装与工具事件清单".into(),
        },
    ))
}

fn visible_review_matches(original: &str, visible: &str) -> bool {
    if original == visible {
        return true;
    }
    // The observed Qoder Markdown renderer displays a doubled backslash as one.
    // Accept this exact presentation change only for a valid single-line JSON
    // object. Parsing and voting always consume the authenticated original text.
    original.starts_with('{')
        && !original.contains(['\r', '\n'])
        && serde_json::from_str::<serde_json::Value>(original).is_ok()
        && original.replace("\\\\", "\\") == visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_backslash_presentation_preserves_authenticated_json_for_parsing() {
        let original = r#"{"request":"request","ballot":{"vote":"pass","reason":"Uses /\\s+/"}}"#;
        let visible = original.replace("\\\\", "\\");
        assert!(serde_json::from_str::<serde_json::Value>(&visible).is_err());
        let mut rows = messages(json!([]));
        rows[1] = row(
            "assistant",
            json!({"role":"assistant","turnId":"turn",
            "text":format!("{original}\n\nEND"),"tools":[],"parts":[]}),
        );
        let (retained, audit) =
            audit_rows("request", "approved packet", &visible, "END", &rows).unwrap();
        assert_eq!(retained, original);
        assert_eq!(audit.status, ReviewToolStatus::Clean);
        assert!(serde_json::from_str::<serde_json::Value>(&retained).is_ok());
        assert!(audit_rows(
            "request",
            "approved packet",
            &visible.replace("pass", "fail"),
            "END",
            &rows
        )
        .is_err());
        assert!(!visible_review_matches(
            r#"{"bad":"\q\\"}"#,
            r#"{"bad":"\q\"}"#
        ));
        assert!(!visible_review_matches(
            "{\n\"text\":\"\\\\\"}",
            "{\n\"text\":\"\\\"}"
        ));
    }

    fn session_database(workspace: &Path, relative: &str, cwd: &Path) {
        let path = workspace.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "CREATE TABLE chat_sessions(session_id TEXT,cwd TEXT,model TEXT,deleted_at TEXT);
             CREATE TABLE chat_session_messages(session_id TEXT,sequence INTEGER,message_id TEXT,payload_hash TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO chat_sessions VALUES (?1,?2,?3,NULL)",
            rusqlite::params!["review-session", cwd.to_str().unwrap(), "byok:test-model"],
        )
        .unwrap();
    }

    #[test]
    fn discovers_standard_and_legacy_isolated_qoder_profiles() {
        for relative in [
            ".cerulean-runtime/home/AppData/Roaming/com.qoder.app.stable/main.sqlite",
            ".cerulean-runtime/appdata/com.qoder.app.stable/main.sqlite",
            ".cerulean-runtime/desktop/main.sqlite",
            ".cerulean-runtime/desktop/desktop-data/main.sqlite",
            ".cerulean-runtime/appdata/Qoder/main.sqlite",
        ] {
            let workspace = tempfile::tempdir().unwrap();
            session_database(workspace.path(), relative, workspace.path());
            let audit = Audit::begin(workspace.path(), "review-session").unwrap();
            assert_eq!(audit.database, workspace.path().join(relative));
            assert_eq!(audit.model, "byok:test-model");
            assert!(audit.prefix.is_empty());
        }
    }

    #[test]
    fn rejects_ambiguous_missing_and_foreign_workspace_profiles() {
        let workspace = tempfile::tempdir().unwrap();
        assert!(Audit::begin(workspace.path(), "review-session").is_err());
        for relative in [
            ".cerulean-runtime/home/AppData/Roaming/com.qoder.app.stable/main.sqlite",
            ".cerulean-runtime/appdata/com.qoder.app.stable/main.sqlite",
        ] {
            session_database(workspace.path(), relative, workspace.path());
        }
        assert!(Audit::begin(workspace.path(), "review-session").is_err());
        let other = tempfile::tempdir().unwrap();
        session_database(
            other.path(),
            ".cerulean-runtime/home/AppData/Roaming/com.qoder.app.stable/main.sqlite",
            workspace.path(),
        );
        assert!(Audit::begin(other.path(), "review-session").is_err());
    }
    fn row(role: &str, value: serde_json::Value) -> Row {
        use sha2::Digest;
        let raw = value.to_string();
        (
            role.into(),
            "completed".into(),
            raw.clone(),
            format!("{:x}", sha2::Sha256::digest(raw.as_bytes())),
            raw.len() as i64,
            1,
            Some("turn".into()),
        )
    }
    fn messages(tools: serde_json::Value) -> Vec<Row> {
        vec![
            row(
                "user",
                json!({"role":"user","turnId":"turn","text":"approved packet","tools":[]}),
            ),
            row(
                "assistant",
                json!({"role":"assistant","turnId":"turn","text":"vote-json END","tools":tools,"parts":[{"type":"thinking","text":"private reasoning"},{"type":"text","text":"vote-json END"},{"type":"hook","hook":{"event":"Stop","status":"succeeded"}}]}),
            ),
        ]
    }
    fn version_two_rows(user: serde_json::Value, assistant: serde_json::Value) -> Vec<Row> {
        [("user", user), ("assistant", assistant)]
            .into_iter()
            .map(|(role, value)| {
                let mut record = row(role, value);
                record.5 = 2;
                record
            })
            .collect()
    }
    fn version_two_payloads() -> (serde_json::Value, serde_json::Value) {
        (
            json!({"issueId":null,"attachments":[],"selectedSkillNames":[],"selectedPluginIds":[],"selectedConnectorIds":[],"selectedAgentNames":[],"selectedCapabilityCommands":[],"referencedChatSessions":[],"parts":[{"id":"turn:text","type":"text","text":"approved packet"}]}),
            json!({"requestSetId":"turn","completedAt":"2026-11-07T00:00:00Z","parts":[{"id":"submit","type":"hook","hook":{"event":"UserPromptSubmit","status":"failed"}},{"id":"reason","type":"thinking","text":"synthetic reasoning","parentToolUseId":null},{"id":"final","type":"text","text":"vote-json END","parentToolUseId":null},{"id":"stop","type":"hook","hook":{"event":"Stop","status":"failed"}}],"finalTextId":"final"}),
        )
    }
    #[test]
    fn version_two_event_parts_detect_tools_and_nested_tool_results() {
        let (user, assistant) = version_two_payloads();
        let rows = version_two_rows(user.clone(), assistant.clone());
        assert_eq!(
            audit_rows("request", "approved packet", "vote-json", "END", &rows)
                .unwrap()
                .1
                .status,
            ReviewToolStatus::Clean
        );
        for tool in [
            json!({"id":"tool","type":"tool","name":"Read"}),
            json!({"id":"tool","type":"hook","hook":{"event":"PreToolUse","status":"failed"}}),
            json!({"id":"tool","type":"text","text":"nested result","parentToolUseId":"call"}),
        ] {
            let mut changed = assistant.clone();
            changed["parts"].as_array_mut().unwrap().insert(1, tool);
            let rows = version_two_rows(user.clone(), changed);
            assert_eq!(
                audit_rows("request", "approved packet", "vote-json", "END", &rows)
                    .unwrap()
                    .1
                    .status,
                ReviewToolStatus::ToolCallObserved
            );
        }
    }
    #[test]
    fn version_two_rejects_extra_inputs_foreign_turns_and_unknown_events() {
        let (user, assistant) = version_two_payloads();
        for field in [
            "attachments",
            "selectedSkillNames",
            "selectedPluginIds",
            "selectedConnectorIds",
            "selectedAgentNames",
            "selectedCapabilityCommands",
            "referencedChatSessions",
        ] {
            let mut changed = user.clone();
            changed[field] = json!(["extra"]);
            assert!(audit_rows(
                "request",
                "approved packet",
                "vote-json",
                "END",
                &version_two_rows(changed, assistant.clone())
            )
            .is_err());
        }
        for mutation in 0..8 {
            let mut changed = assistant.clone();
            match mutation {
                0 => changed["requestSetId"] = json!("foreign"),
                1 => changed["finalTextId"] = json!("missing"),
                2 => changed["parts"][0]["hook"]["command"] = json!("unknown-command"),
                3 => changed["parts"][0]["hook"]["event"] = json!("FutureHook"),
                4 => changed["parts"][1]["type"] = json!("future-event"),
                5 => changed["parts"][1]["id"] = json!("final"),
                6 => changed["tools"] = json!([]),
                _ => changed["parts"][2]["tool"] = json!({"name":"Read"}),
            }
            assert!(
                audit_rows(
                    "request",
                    "approved packet",
                    "vote-json",
                    "END",
                    &version_two_rows(user.clone(), changed)
                )
                .is_err(),
                "mutation {mutation}"
            );
        }
        let mut rows = version_two_rows(user, assistant);
        rows[1].6 = Some("foreign".into());
        assert!(audit_rows("request", "approved packet", "vote-json", "END", &rows).is_err());
        rows[1].6 = None;
        assert!(audit_rows("request", "approved packet", "vote-json", "END", &rows).is_err());
    }
    #[test]
    fn version_two_audit_reads_only_new_complete_database_events() {
        let workspace = tempfile::tempdir().unwrap();
        let relative = ".cerulean-runtime/home/AppData/Roaming/com.qoder.app.stable/main.sqlite";
        session_database(workspace.path(), relative, workspace.path());
        let audit = Audit::begin(workspace.path(), "review-session").unwrap();
        let db = Connection::open(workspace.path().join(relative)).unwrap();
        db.execute_batch(
            "ALTER TABLE chat_session_messages ADD COLUMN role TEXT;
            ALTER TABLE chat_session_messages ADD COLUMN status TEXT;
            ALTER TABLE chat_session_messages ADD COLUMN payload_json TEXT;
            ALTER TABLE chat_session_messages ADD COLUMN payload_bytes INTEGER;
            ALTER TABLE chat_session_messages ADD COLUMN payload_schema_version INTEGER;
            ALTER TABLE chat_session_messages ADD COLUMN turn_id TEXT;",
        )
        .unwrap();
        let (user, assistant) = version_two_payloads();
        for (index, record) in version_two_rows(user, assistant).into_iter().enumerate() {
            db.execute(
                "INSERT INTO chat_session_messages VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    "review-session",
                    index as i64,
                    format!("message-{index}"),
                    record.3,
                    record.0,
                    record.1,
                    record.2,
                    record.4,
                    record.5,
                    record.6
                ],
            )
            .unwrap();
        }
        assert_eq!(
            audit
                .finish("request", "approved packet", "vote-json", "END")
                .unwrap()
                .1
                .status,
            ReviewToolStatus::Clean
        );
    }
    #[test]
    fn complete_qoder_envelopes_distinguish_structural_calls_from_quoted_text() {
        let audit = audit_rows(
            "request",
            "approved packet",
            "vote-json",
            "END",
            &messages(json!([])),
        )
        .unwrap();
        assert_eq!(audit.1.status, ReviewToolStatus::Clean);
        let audit = audit_rows(
            "request",
            "approved packet",
            "vote-json",
            "END",
            &messages(json!([{"name":"Read","input":{"path":"synthetic"}}])),
        )
        .unwrap();
        assert_eq!(audit.1.status, ReviewToolStatus::ToolCallObserved);
        assert_eq!(audit.1.tools, vec!["Read"]);
        let mut rows = messages(json!([]));
        rows[1] = row(
            "assistant",
            json!({"role":"assistant","turnId":"turn","text":"vote-json END","tools":[],"parts":[{"type":"text","text":"Quoted data: tools Read Bash"}]}),
        );
        assert_eq!(
            audit_rows("request", "approved packet", "vote-json", "END", &rows)
                .unwrap()
                .1
                .status,
            ReviewToolStatus::Clean
        );
        rows[1] = row(
            "assistant",
            json!({"role":"assistant","turnId":"turn","text":"vote-json END","tools":[],"parts":[{"type":"tool","id":"call"}]}),
        );
        assert_eq!(
            audit_rows("request", "approved packet", "vote-json", "END", &rows)
                .unwrap()
                .1
                .status,
            ReviewToolStatus::ToolCallObserved
        );
    }
    #[test]
    fn incomplete_tampered_foreign_and_unsupported_qoder_records_cannot_pass() {
        let mut rows = messages(json!([]));
        rows[1].3 = "a".repeat(64);
        assert!(audit_rows("request", "approved packet", "vote-json", "END", &rows).is_err());
        let mut rows = messages(json!([]));
        rows[1].1 = "streaming".into();
        assert!(audit_rows("request", "approved packet", "vote-json", "END", &rows).is_err());
        for changed in [
            json!({"role":"assistant","turnId":"foreign","text":"vote-json END","tools":[],"parts":[]}),
            json!({"role":"assistant","turnId":"turn","text":"vote-json END","parts":[]}),
            json!({"role":"assistant","turnId":"turn","text":"vote-json END","tools":[],"parts":[{"type":"future-event"}]}),
        ] {
            let mut rows = messages(json!([]));
            rows[1] = row("assistant", changed);
            assert!(audit_rows("request", "approved packet", "vote-json", "END", &rows).is_err());
        }
        assert!(audit_rows(
            "request",
            "wrong-packet",
            "vote-json",
            "END",
            &messages(json!([]))
        )
        .is_err());
    }
}
