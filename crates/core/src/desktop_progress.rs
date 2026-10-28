//! Read-only, bounded progress samples from the original desktop task owner.
//! No workflow writes, model requests, stop capability or historical replay.
use super::*;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Instant,
};
const INTERVAL: Duration = Duration::from_secs(5);
const MAX_SAMPLES: u32 = 24;

pub(super) fn validate(
    review: &DesktopTaskReview,
    sample: &DesktopTaskProgress,
) -> Result<(), String> {
    let mut unhashed = review.clone();
    unhashed.id.clear();
    require(
        digest(&unhashed)? == review.id && review.bridge_sha256 == fingerprint(),
        "进度观察缺少原批准桥接与任务身份",
    )?;
    crate::desktop_execution::validate_read_task_window(&review.observation.window)?;
    let original = &review.observation;
    let observed = &sample.observation;
    require(
        sample.schema_version == 1
            && sample.task_review_digest == review.id
            && observed.window == original.window
            && observed.document_runtime_id == original.document_runtime_id
            && observed.session_id == original.session_id
            && observed.model_label == original.model_label
            && observed.draft_empty
            && (1..=1000).contains(&observed.node_count),
        "进度观察的窗口、聊天、模型或草稿已变化",
    )?;
    for hash in [
        &observed.content_digest,
        &observed.layout_digest,
        &sample.visible_digest,
    ] {
        sha256(hash)?;
    }
    require(
        sample.baseline.len() >= 2
            && sample.baseline.len() <= review.baseline.len()
            && sample.baseline == review.baseline[review.baseline.len() - sample.baseline.len()..],
        "进度观察不属于原历史后缀",
    )?;
    for id in [&sample.turn_runtime_id, &sample.prompt_runtime_id] {
        require(
            !id.trim().is_empty() && id.len() <= 512 && !id.chars().any(char::is_control),
            "进度观察轮次身份无效",
        )?;
    }
    require(
        sample.turn_runtime_id != sample.prompt_runtime_id
            && sample.status_text.as_ref().is_none_or(|s| {
                !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control)
            }),
        "进度观察状态无效",
    )?;
    let at = chrono::DateTime::parse_from_rfc3339(&observed.captured_at)
        .map_err(|_| "进度观察时间无效")?;
    let prior = chrono::DateTime::parse_from_rfc3339(&original.captured_at)
        .map_err(|_| "原任务时间无效")?;
    let age = chrono::Utc::now() - at.with_timezone(&chrono::Utc);
    require(
        at >= prior && age >= chrono::Duration::zero() && age <= chrono::Duration::seconds(30),
        "进度观察已过期或早于原任务",
    )
}

struct Probe {
    receiver: mpsc::Receiver<Result<DesktopTaskProgress, String>>,
    thread: JoinHandle<()>,
    cancel: Arc<AtomicBool>,
}
pub(super) struct Tracker {
    probe: Option<Probe>,
    next: Instant,
    attempts: u32,
    original_turn: Option<(String, String)>,
}
impl Default for Tracker {
    fn default() -> Self {
        Self {
            probe: None,
            next: Instant::now() + INTERVAL,
            attempts: 0,
            original_turn: None,
        }
    }
}
impl Drop for Tracker {
    fn drop(&mut self) {
        if let Some(probe) = self.probe.take() {
            probe.cancel.store(true, Ordering::Release);
            let _ = probe.thread.join();
        }
    }
}
impl Tracker {
    pub(super) fn idle(&self) -> bool {
        self.probe.is_none()
    }

    pub(super) fn poll(
        &mut self,
        root: &Path,
        status: &mut DesktopTaskStatus,
        stopping: bool,
        #[cfg(test)] fixture: Option<Fixture>,
    ) {
        if let Some(probe) = &self.probe {
            if stopping {
                probe.cancel.store(true, Ordering::Release);
            }
            if !probe.thread.is_finished() {
                return;
            }
            let probe = self.probe.take().unwrap();
            let joined = probe.thread.join().is_ok();
            let result = if joined {
                probe
                    .receiver
                    .try_recv()
                    .unwrap_or_else(|_| Err("进度观察通道已关闭".into()))
            } else {
                Err("进度观察后台异常".into())
            };
            let view = status.progress.get_or_insert_with(Default::default);
            view.checking = false;
            if stopping {
                return;
            }
            let result = result.and_then(|sample| {
                validate(&status.review, &sample)?;
                let identity = (
                    sample.turn_runtime_id.clone(),
                    sample.prompt_runtime_id.clone(),
                );
                require(
                    self.original_turn
                        .as_ref()
                        .is_none_or(|old| old == &identity),
                    "原任务的可见轮次已变化",
                )?;
                if let Some(old) = &view.latest {
                    require(
                        chrono::DateTime::parse_from_rfc3339(&sample.observation.captured_at)
                            .map_err(|_| "进度观察时间无效")?
                            >= chrono::DateTime::parse_from_rfc3339(&old.observation.captured_at)
                                .map_err(|_| "前次观察时间无效")?,
                        "进度观察时间倒退",
                    )?;
                }
                self.original_turn = Some(identity);
                Ok(sample)
            });
            match result {
                Ok(sample) => {
                    view.latest = Some(sample);
                    view.samples += 1;
                    view.error = None;
                }
                Err(error) => {
                    view.error = Some(error);
                }
            }
            self.next = Instant::now() + INTERVAL;
        }
        if stopping {
            return;
        }
        if self.attempts >= MAX_SAMPLES {
            let view = status.progress.get_or_insert_with(Default::default);
            view.checking = false;
            view.error.get_or_insert_with(|| {
                "本次自动进度观察已达到上限，后续界面状态不再自动更新".into()
            });
            return;
        }
        if Instant::now() < self.next {
            return;
        }
        self.attempts += 1;
        self.next = Instant::now() + INTERVAL;
        let root = root.to_owned();
        let review = status.review.clone();
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("desktop-progress".into())
            .spawn(move || {
                let request = ReadRequest::TaskProgress { version: 1, review };
                #[cfg(test)]
                let result = if let Some(f) = fixture {
                    f(&request)
                } else {
                    observe_cancellable(&root, &request, &worker_cancel)
                };
                #[cfg(not(test))]
                let result = observe_cancellable(&root, &request, &worker_cancel);
                let _ = sender.send(
                    result.and_then(|reply| reply.task_progress.ok_or("原任务进度观察缺失".into())),
                );
            });
        let view = status.progress.get_or_insert_with(Default::default);
        match thread {
            Ok(thread) => {
                self.probe = Some(Probe {
                    receiver,
                    thread,
                    cancel,
                });
                view.checking = true;
            }
            Err(_) => {
                view.checking = false;
                view.error = Some("进度观察后台未启动".into());
            }
        }
    }
}

#[cfg(test)]
#[path = "desktop_progress_tests.rs"]
mod tests;
