//! Window-scoped exit fences. Reads as well as writes can start the resident,
//! so every host entry holds a permit until its original operation completes.
use std::{
    collections::BTreeMap,
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

#[derive(Default)]
pub struct Lifecycle(Mutex<BTreeMap<String, Arc<Gate>>>);
#[derive(Default)]
pub struct Gate {
    closing: AtomicBool,
    stopped: AtomicBool,
    operations: Arc<RwLock<()>>,
    exit: tokio::sync::Mutex<()>,
    maintenance: Mutex<Vec<Child>>,
}
impl Lifecycle {
    pub fn gate(&self, label: &str) -> Result<Arc<Gate>, String> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "退出状态不可用")?
            .entry(label.into())
            .or_default()
            .clone())
    }
    pub async fn enter(&self, label: &str) -> Result<OwnedRwLockReadGuard<()>, String> {
        self.gate(label)?.enter().await
    }
}
impl Gate {
    pub async fn enter(&self) -> Result<OwnedRwLockReadGuard<()>, String> {
        if self.closing.load(Ordering::SeqCst) {
            return Err("正在退出工作台，已暂停新的操作与自动连接".into());
        }
        let permit = self.operations.clone().read_owned().await;
        if self.closing.load(Ordering::SeqCst) {
            return Err("正在退出工作台，已暂停新的操作与自动连接".into());
        }
        Ok(permit)
    }
    pub fn reopen(&self) -> Result<(), String> {
        let _exit = self
            .exit
            .try_lock()
            .map_err(|_| "后台退出检查尚未返回，请等待原结果")?;
        self.stopped.store(false, Ordering::SeqCst);
        self.closing.store(false, Ordering::SeqCst);
        Ok(())
    }
    pub fn track_maintenance(&self, child: Child) -> Result<(), String> {
        self.maintenance
            .lock()
            .map_err(|_| "维护窗口状态不可用")?
            .push(child);
        Ok(())
    }
    fn ensure_maintenance_closed(&self) -> Result<(), String> {
        let mut children = self.maintenance.lock().map_err(|_| "维护窗口状态不可用")?;
        let mut pending = vec![];
        for (index, child) in children.iter_mut().enumerate() {
            if child
                .try_wait()
                .map_err(|e| format!("无法核对维护窗口退出：{e}"))?
                .is_none()
            {
                pending.push(index);
            }
        }
        if !pending.is_empty() {
            return Err(
                "请先正常关闭本工作台打开的独立维护窗口，再确认退出；维护操作不会被强制终止".into(),
            );
        }
        children.clear();
        Ok(())
    }
    async fn drain(&self) -> Result<OwnedRwLockWriteGuard<()>, String> {
        self.closing.store(true, Ordering::SeqCst);
        tokio::time::timeout(
            Duration::from_secs(35),
            self.operations.clone().write_owned(),
        )
        .await
        .map_err(|_| "仍有界面操作尚未返回，窗口已保留；退出请求未发送".into())
    }
    pub async fn stop(&self, directory: &std::path::Path, id: String) -> Result<(), String> {
        let _exit = self
            .exit
            .try_lock()
            .map_err(|_| "正在核对原退出请求，请勿重复确认")?;
        let _drained = self.drain().await?;
        self.ensure_maintenance_closed()?;
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(());
        }
        #[cfg(windows)]
        cerulean_core::ipc::Client::stop_for_window(directory, id).await?;
        #[cfg(not(windows))]
        {
            let _ = (directory, id);
            return Err("后台退出检查需要 Windows".into());
        }
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "Owned child fixture, launched only by the parent exit test"]
    fn idle_owned_core_fixture() {
        let path = std::path::PathBuf::from(
            std::env::var_os("CERULEAN_EXIT_FIXTURE_ROOT").expect("owned fixture root"),
        );
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(cerulean_core::ipc::serve(&path))
            .unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn window_exit_stops_authenticated_original_process_without_restarting_it() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let root = tempfile::tempdir().unwrap();
        struct Owned(Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let mut child = Owned(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "workbench_exit::tests::idle_owned_core_fixture",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("CERULEAN_EXIT_FIXTURE_ROOT", root.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap(),
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let client = loop {
            if let Ok(client) = cerulean_core::ipc::Client::connect(root.path()).await {
                break client;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "fixture exited before readiness"
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "fixture startup timed out"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        assert_eq!(client.server_pid, child.0.id());
        drop(client);
        let gate = Gate::default();
        gate.stop(root.path(), "owned-window-stop".into())
            .await
            .unwrap();
        assert!(child.0.try_wait().unwrap().unwrap().success());
        assert!(
            gate.enter().await.is_err(),
            "a late refresh must not restart the core"
        );
        assert_eq!(
            cerulean_core::ipc::Client::stop_for_window(root.path(), "already-stopped".into())
                .await
                .unwrap(),
            None
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn absent_pipe_with_active_writer_blocks_exit_and_keeps_refresh_fenced() {
        let root = tempfile::tempdir().unwrap();
        let writer = cerulean_core::Store::open(root.path()).unwrap();
        let gate = Gate::default();
        assert!(gate
            .stop(root.path(), "busy-writer".into())
            .await
            .unwrap_err()
            .contains("占用"));
        assert!(gate.enter().await.is_err());
        drop(writer);
        // Explicit user action may re-check; no automatic retry happened.
        gate.stop(root.path(), "user-rechecked".into())
            .await
            .unwrap();
        assert!(gate.enter().await.is_err());
    }

    #[test]
    #[ignore = "Owned short-lived maintenance child for the parent test"]
    fn maintenance_child_fixture() {
        std::thread::sleep(Duration::from_millis(700));
    }

    #[tokio::test]
    async fn open_maintenance_child_blocks_exit_without_terminating_it() {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "workbench_exit::tests::maintenance_child_fixture",
                "--ignored",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let gate = Gate::default();
        gate.track_maintenance(command.spawn().unwrap()).unwrap();
        assert!(gate
            .ensure_maintenance_closed()
            .unwrap_err()
            .contains("维护窗口"));
        assert!(gate.maintenance.lock().unwrap()[0]
            .wait()
            .unwrap()
            .success());
        gate.ensure_maintenance_closed().unwrap();
    }
    #[tokio::test]
    async fn close_fences_late_reads_drains_original_and_keeps_other_workbench_open() {
        let lifecycle = Lifecycle::default();
        let main = lifecycle.gate("main").unwrap();
        let original = lifecycle.enter("main").await.unwrap();
        let draining = main.clone();
        let task = tokio::spawn(async move { draining.drain().await.unwrap() });
        while !main.closing.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        assert!(lifecycle.enter("main").await.is_err());
        assert!(lifecycle.enter("experiment-a").await.is_ok());
        assert!(!task.is_finished());
        drop(original);
        drop(task.await.unwrap());
        assert!(lifecycle.enter("main").await.is_err());
        main.reopen().unwrap();
        assert!(lifecycle.enter("main").await.is_ok());
    }
    #[tokio::test]
    async fn duplicate_exit_and_cancel_cannot_race_original_stop() {
        let gate = Gate::default();
        let _exit = gate.exit.lock().await;
        assert!(gate.reopen().is_err());
        assert!(gate
            .stop(std::path::Path::new("unused"), "duplicate".into())
            .await
            .is_err());
    }
}
