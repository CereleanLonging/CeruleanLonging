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
    ) -> Result<ReviewToolAudit, String> {
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
        let mut statement = transaction.prepare("SELECT CASE WHEN length(role)<=32 THEN role ELSE NULL END,CASE WHEN length(status)<=32 THEN status ELSE NULL END,CASE WHEN length(CAST(payload_json AS BLOB))<=524288 THEN payload_json ELSE NULL END,CASE WHEN length(payload_hash)=64 THEN payload_hash ELSE NULL END,payload_bytes,payload_schema_version FROM chat_session_messages WHERE session_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 65").map_err(|_|"Qoder评分事件结构不支持")?;
        let rows = statement
            .query_map(rusqlite::params![self.session, after], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|_| "Qoder评分事件不可读取")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Qoder评分事件不完整")?;
        audit_rows(request, prompt, reply, marker, &rows)
    }
}
type Row = (String, String, String, String, i64, i64);
fn audit_rows(
    request: &str,
    prompt: &str,
    reply: &str,
    marker: &str,
    rows: &[Row],
) -> Result<ReviewToolAudit, String> {
    use sha2::Digest;
    require((2..=64).contains(&rows.len()), "Qoder评分消息缺失或超限")?;
    let mut turn = None;
    let mut user_seen = false;
    let mut last_text = None;
    let mut tools = vec![];
    let mut total = 0usize;
    for (role, status, raw, hash, bytes, schema) in rows {
        total = total.checked_add(raw.len()).ok_or("Qoder评分数据溢出")?;
        require(
            total <= 512 * 1024
                && *schema == 1
                && *bytes == raw.len() as i64
                && format!("{:x}", sha2::Sha256::digest(raw.as_bytes())) == *hash
                && status == "completed",
            "Qoder评分封装不完整或摘要不符",
        )?;
        let message: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| "Qoder评分JSON无效")?;
        require(message["role"].as_str() == Some(role), "Qoder评分角色不符")?;
        let message_turn = message["turnId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Qoder评分轮次缺失")?;
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
    require(
        last_text
            .as_deref()
            .and_then(|t| t.trim().strip_suffix(marker))
            .map(str::trim)
            == Some(reply),
        "Qoder评分记录与原窗口回复不符",
    )?;
    let evidence_digest = digest(&(request, rows.iter().map(|r| &r.3).collect::<Vec<_>>()))?;
    Ok(ReviewToolAudit {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        assert_eq!(audit.status, ReviewToolStatus::Clean);
        let audit = audit_rows(
            "request",
            "approved packet",
            "vote-json",
            "END",
            &messages(json!([{"name":"Read","input":{"path":"synthetic"}}])),
        )
        .unwrap();
        assert_eq!(audit.status, ReviewToolStatus::ToolCallObserved);
        assert_eq!(audit.tools, vec!["Read"]);
        let mut rows = messages(json!([]));
        rows[1] = row(
            "assistant",
            json!({"role":"assistant","turnId":"turn","text":"vote-json END","tools":[],"parts":[{"type":"text","text":"Quoted data: tools Read Bash"}]}),
        );
        assert_eq!(
            audit_rows("request", "approved packet", "vote-json", "END", &rows)
                .unwrap()
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
