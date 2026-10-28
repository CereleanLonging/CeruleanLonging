//! Offline evidence operations. No runtime process, IPC, or workbench data access.
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Request {
    Create {
        root: PathBuf,
        index: PathBuf,
        index_sha256: String,
        destination: PathBuf,
    },
    Verify {
        archive: PathBuf,
        manifest_sha256: String,
    },
}
pub struct Service(Arc<Semaphore>);
impl Default for Service {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(1)))
    }
}
impl Service {
    async fn blocking(
        &self,
        work: impl FnOnce() -> Result<serde_json::Value, String> + Send + 'static,
    ) -> Result<serde_json::Value, String> {
        let permit = self
            .0
            .clone()
            .try_acquire_owned()
            .map_err(|_| "原归档操作仍在进行，请等待；未排队或重复执行")?;
        // The worker owns the permit even if the invoking window disappears.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|e| format!("归档操作结果未确认，保留原目录，未重试：{e}"))?
    }
    pub async fn run(&self, request: Request) -> Result<serde_json::Value, String> {
        self.blocking(move || {
            #[cfg(windows)]
            {
                use cerulean_core::portable_evidence;
                let result = match request {
                    Request::Create {
                        root,
                        index,
                        index_sha256,
                        destination,
                    } => portable_evidence::create(&root, &index, &index_sha256, &destination),
                    Request::Verify {
                        archive,
                        manifest_sha256,
                    } => portable_evidence::verify(&archive, &manifest_sha256),
                }?;
                serde_json::to_value(result).map_err(|e| e.to_string())
            }
            #[cfg(not(windows))]
            {
                let _ = request;
                Err("证据归档首版仅支持Windows".into())
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_rejects_implicit_digests_and_execution_fields() {
        for raw in [
            r#"{"action":"verify","archive":"D:/archive"}"#,
            r#"{"action":"verify","archive":"D:/archive","manifestSha256":"x","execute":true}"#,
            r#"{"action":"create","root":"D:/source","index":"D:/index","destination":"D:/new"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(raw).is_err());
        }
        assert!(serde_json::from_str::<Request>(
            r#"{"action":"verify","archive":"D:/archive","manifestSha256":"x"}"#
        )
        .is_ok());
    }
    #[tokio::test]
    async fn closing_observer_does_not_release_worker_gate_or_queue_a_second_operation() {
        let service = Arc::new(Service::default());
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let original = service.clone();
        let waiter = tokio::spawn(async move {
            original
                .blocking(move || {
                    started.send(()).unwrap();
                    hold.recv().unwrap();
                    Ok(serde_json::json!({"done":true}))
                })
                .await
        });
        ready.await.unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert!(service
            .blocking(|| panic!("must never run or queue"))
            .await
            .unwrap_err()
            .contains("仍在进行"));
        release.send(()).unwrap();
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.0.clone().acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(permit);
        assert_eq!(
            service
                .blocking(|| Ok(serde_json::json!("available")))
                .await
                .unwrap(),
            "available"
        );
    }
}
