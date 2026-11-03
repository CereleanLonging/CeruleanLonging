//! Negotiated transport for user-authored documents only. Ordinary IPC budgets stay intact.
use super::*;
const STREAM: u32 = u32::MAX;

pub(super) fn document_operation(operation: &UserOperation) -> bool {
    matches!(
        operation,
        UserOperation::Command {
            request: Request {
                command: Command::CreatePlan { .. } | Command::ConfigureProject { .. },
                ..
            }
        } | UserOperation::Request {
            request: Request {
                command: Command::Snapshot,
                ..
            }
        } | UserOperation::Workspace {
            query: cerulean_domain::workspace::WorkspaceQuery::Plan { .. }
                | cerulean_domain::workspace::WorkspaceQuery::Configuration { .. },
            ..
        }
    )
}

pub(super) async fn write<W: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut W,
    value: &T,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    stream
        .write_u32_le(STREAM)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_u64_le(bytes.len() as u64)
        .await
        .map_err(|e| e.to_string())?;
    for part in bytes.chunks(REQUEST_LIMIT) {
        write_frame_bytes(stream, part, REQUEST_LIMIT).await?;
    }
    stream.write_u32_le(0).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())
}

pub(super) async fn read<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    stream: &mut R,
    ordinary_limit: usize,
) -> Result<(T, bool, usize), String> {
    let first = stream.read_u32_le().await.map_err(|e| e.to_string())?;
    let extended = first == STREAM;
    let mut bytes = Vec::new();
    if extended {
        let total = usize::try_from(stream.read_u64_le().await.map_err(|e| e.to_string())?)
            .map_err(|_| "文档超过本机可寻址容量")?;
        if total == 0 {
            return Err("empty document frame".into());
        }
        loop {
            let size = stream.read_u32_le().await.map_err(|e| e.to_string())? as usize;
            if size == 0 {
                break;
            }
            if size > REQUEST_LIMIT || bytes.len().checked_add(size).is_none_or(|n| n > total) {
                return Err("invalid document chunk".into());
            }
            bytes
                .try_reserve(size)
                .map_err(|_| "本机可用内存不足，文档未提交")?;
            let start = bytes.len();
            bytes.resize(start + size, 0);
            stream
                .read_exact(&mut bytes[start..])
                .await
                .map_err(|e| e.to_string())?;
        }
        if bytes.len() != total {
            return Err("incomplete document stream; not submitted".into());
        }
    } else {
        let size = first as usize;
        if size == 0 || size > ordinary_limit {
            return Err("invalid frame length".into());
        }
        bytes.resize(size, 0);
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
    }
    let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((value, extended, bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn large_unicode_document_round_trips_without_changing_ordinary_frame_limits() {
        let original = "文档内容😀\n".repeat(110_000);
        let (mut tx, mut rx) = tokio::io::duplex(4096);
        let wanted = original.clone();
        let sender = tokio::spawn(async move {
            write(&mut tx, &wanted).await.unwrap();
        });
        let (actual, extended, _): (String, _, _) = read(&mut rx, REQUEST_LIMIT).await.unwrap();
        sender.await.unwrap();
        assert!(extended);
        assert_eq!(actual, original);
    }
    #[tokio::test]
    async fn truncated_or_oversized_chunks_are_not_commands() {
        for size in [3, REQUEST_LIMIT + 1] {
            let (mut tx, mut rx) = tokio::io::duplex(4096);
            tx.write_u32_le(STREAM).await.unwrap();
            tx.write_u64_le(2).await.unwrap();
            tx.write_u32_le(size as u32).await.unwrap();
            assert!(read::<_, serde_json::Value>(&mut rx, REQUEST_LIMIT)
                .await
                .is_err());
        }
        let (mut tx, mut rx) = tokio::io::duplex(4096);
        tx.write_u32_le(STREAM).await.unwrap();
        tx.write_u64_le(5).await.unwrap();
        tx.write_u32_le(0).await.unwrap();
        assert!(read::<_, serde_json::Value>(&mut rx, REQUEST_LIMIT)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn real_service_saves_reads_and_reopens_a_document_above_both_frame_limits() {
        use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().to_path_buf();
        let server = tokio::spawn(async move { serve(&directory).await });
        let end = tokio::time::Instant::now() + Duration::from_secs(15);
        let client = loop {
            match Client::connect(root.path()).await {
                Ok(client) => break client,
                Err(_) => {
                    assert!(tokio::time::Instant::now() < end);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        };
        let tasks = vec!["文档完整内容😀".repeat(2500); 100];
        assert!(serde_json::to_vec(&tasks).unwrap().len() > RESPONSE_LIMIT);
        let reply = client
            .call(UserOperation::Command {
                request: Request {
                    id: "large-create".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::CreatePlan {
                        title: "长文档".into(),
                        tasks: tasks.clone(),
                    },
                },
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.command_receipt.is_some());
        let page = Client::connect(root.path())
            .await
            .unwrap()
            .call(UserOperation::Workspace {
                id: "list".into(),
                protocol_version: PROTOCOL_VERSION,
                query: WorkspaceQuery::Plans {
                    offset: 0,
                    revision: None,
                },
            })
            .await
            .unwrap();
        let Some(WorkspaceResult::Plans(page)) = page.workspace else {
            panic!("plan page missing")
        };
        let detail = Client::connect(root.path())
            .await
            .unwrap()
            .call(UserOperation::Workspace {
                id: "detail".into(),
                protocol_version: PROTOCOL_VERSION,
                query: WorkspaceQuery::Plan {
                    id: page.entries[0].id.clone(),
                    revision: page.revision,
                },
            })
            .await
            .unwrap();
        let Some(WorkspaceResult::Plan(plan)) = detail.workspace else {
            panic!("document detail missing: {:?}", detail.response.error)
        };
        assert_eq!(plan.tasks, tasks);
        assert!(!plan.approved);
        let stop = Client::connect(root.path())
            .await
            .unwrap()
            .call(UserOperation::Shutdown {
                id: "stop".into(),
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(stop.stopping);
        server.await.unwrap().unwrap();
        let store = crate::Store::open(root.path()).unwrap();
        assert_eq!(store.snapshot().plans[0].tasks, tasks);
    }
}
