//! Private, single-writer project review ledger. No model-facing filesystem or IPC.
use cerulean_domain::{
    blind_review::*,
    execution::{digest, identifier, require},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub(crate) mod material;
#[cfg(test)]
pub(crate) mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Pending {
    pub epoch: String,
    pub request: String,
    pub seat: String,
    pub readiness: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum AnalysisState {
    Pending {
        request: Pending,
    },
    Completed {
        model: String,
        analysis: FailureAnalysis,
    },
    Failed {
        message: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Round {
    pub id: String,
    pub binding: ReviewBinding,
    pub material: ReviewMaterial,
    pub coverage: ReviewCoverage,
    pub seats: Vec<ReviewSeat>,
    pub digest: String,
    pub phase: ReviewPhase,
    pub deadline: Option<i64>,
    pub deputy_ready: bool,
    pub pending: Option<Pending>,
    pub ballots: BTreeMap<String, RevealedBallot>,
    pub message: Option<String>,
    pub analysis: Option<AnalysisState>,
}
impl Round {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.id)?;
        self.binding.validate()?;
        self.material.validate()?;
        require(
            !self.coverage.files.is_empty()
                && self.coverage.files.len() <= 128
                && self.coverage.excluded.len() <= 128,
            "盲评交付清单无效",
        )?;
        for p in self.coverage.files.iter().chain(&self.coverage.excluded) {
            cerulean_domain::execution::artifact_path(p)?;
        }
        require(
            digest(&self.material)? == self.binding.material,
            "盲评材料摘要不一致",
        )?;
        require(
            (3..=MAX_SEATS).contains(&self.seats.len()),
            "盲评名册必须包含主副指挥及1至5个执行身份",
        )?;
        let mut ids = std::collections::BTreeSet::new();
        let mut workers = std::collections::BTreeSet::new();
        for seat in &self.seats {
            identifier(&seat.id)?;
            seat.target.url()?;
            require(
                ids.insert(&seat.id) && !seat.model.is_empty() && seat.model.len() <= 200,
                "盲评席位重复或模型无效",
            )?;
            require(
                !seat.allowed_models.is_empty()
                    && seat.allowed_models.len() <= 8
                    && seat
                        .allowed_models
                        .iter()
                        .all(|m| !m.is_empty() && m.len() <= 200),
                "评审模型解析规则无效",
            )?;
            require(
                matches!(seat.role.as_str(), "chief" | "deputy" | "worker"),
                "评审角色无效",
            )?;
            if seat.role == "worker" {
                require(
                    workers.insert(digest(&(&seat.target, &seat.model))?),
                    "执行评审身份重复",
                )?;
            }
        }
        for role in ["chief", "deputy"] {
            require(
                self.seats.iter().filter(|s| s.role == role).count() == 1,
                "主副指挥各需一席",
            )?;
        }
        require(
            self.digest == review_digest(&self.binding, &self.seats)?,
            "盲评批准摘要不一致",
        )?;
        if let Some(p) = &self.pending {
            cerulean_domain::execution::sha256(&p.epoch)?;
            identifier(&p.request)?;
            require(
                self.phase == ReviewPhase::Collecting && self.seats.iter().any(|s| s.id == p.seat),
                "盲评未决请求身份或状态错误",
            )?;
        }
        require(
            self.phase != ReviewPhase::Collecting || self.deadline.is_some(),
            "盲评缺少整轮期限",
        )?;
        for (id, ballot) in &self.ballots {
            let seat = self
                .seats
                .iter()
                .find(|s| &s.id == id)
                .ok_or("票据席位缺失")?;
            require(
                ballot.seat == *id && seat.allowed_models.contains(&ballot.model),
                "票据模型身份不符",
            )?;
            ballot.ballot.validate(&self.material)?;
        }
        if matches!(
            self.phase,
            ReviewPhase::AwaitingUser | ReviewPhase::Accepted | ReviewPhase::Rejected
        ) {
            require(
                self.deputy_ready
                    && self.ballots.len() == self.seats.len()
                    && self.pending.is_none(),
                "完整票集缺失",
            )?;
            let all_pass = self.ballots.values().all(|b| b.ballot.vote == Vote::Pass);
            require(
                all_pass == (self.phase != ReviewPhase::Rejected),
                "盲评结果与全票门槛不符",
            )?;
        }
        if let Some(analysis) = &self.analysis {
            require(
                matches!(
                    self.phase,
                    ReviewPhase::Rejected | ReviewPhase::ChangesRequested
                ),
                "仅完整拒绝结果可交副指挥分析",
            )?;
            require(
                self.ballots.len() == self.seats.len()
                    && self.ballots.values().any(|b| b.ballot.vote == Vote::Fail),
                "副指挥不能分析部分票集",
            )?;
            match analysis {
                AnalysisState::Pending { request } => {
                    require(
                        self.phase == ReviewPhase::Rejected
                            && !request.readiness
                            && self
                                .seats
                                .iter()
                                .any(|s| s.id == request.seat && s.role == "deputy"),
                        "副指挥分析身份无效",
                    )?;
                    identifier(&request.request)?;
                    cerulean_domain::execution::sha256(&request.epoch)?;
                }
                AnalysisState::Completed { model, analysis } => {
                    require(
                        self.seats
                            .iter()
                            .any(|s| s.role == "deputy" && s.allowed_models.contains(model)),
                        "副指挥分析模型不符",
                    )?;
                    analysis.validate()?;
                }
                AnalysisState::Failed { message } => require(
                    !message.is_empty() && message.len() <= 4096,
                    "副指挥分析错误说明无效",
                )?,
            }
        }
        Ok(())
    }
    pub fn view(&self) -> BlindReviewView {
        // Every complete ballot set is revealed by the same commit that adds
        // the final vote. Later cancellation must not hide that public history.
        let revealed = self.deputy_ready && self.ballots.len() == self.seats.len();
        BlindReviewView {
            project_id: self.binding.project_id.clone(),
            round_id: self.id.clone(),
            digest: self.digest.clone(),
            phase: self.phase,
            seats: self.seats.clone(),
            file_count: self
                .material
                .fragments
                .iter()
                .map(|f| &f.file)
                .collect::<std::collections::BTreeSet<_>>()
                .len() as u32,
            fragment_count: self.material.fragments.len() as u32,
            material_bytes: serde_json::to_vec(&self.material).map_or(0, |b| b.len() as u32),
            max_requests: self.seats.len() as u32 + 2,
            max_output_tokens: 4096,
            timeout_seconds: 90,
            deadline: self
                .deadline
                .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
                .map(|d| d.to_rfc3339()),
            ballots: revealed.then(|| self.ballots.values().cloned().collect()),
            message: self.message.clone(),
            material_preview: (self.phase == ReviewPhase::Prepared).then(|| self.material.clone()),
            coverage: self.coverage.clone(),
            is_current: false,
            final_accepted: false,
            deputy_analysis: match &self.analysis {
                Some(AnalysisState::Completed { analysis, .. }) => Some(analysis.clone()),
                _ => None,
            },
            analysis_status: self.analysis.as_ref().map(|a| match a {
                AnalysisState::Pending { .. } => "副指挥正在分析已揭晓结果".into(),
                AnalysisState::Completed { .. } => {
                    "副指挥已提出主指挥修订请求，请审阅后进入方案问答".into()
                }
                AnalysisState::Failed { message } => message.clone(),
            }),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    sequence: u32,
    previous: String,
    round: Round,
    hash: String,
}
pub(crate) struct Ledger {
    folder: PathBuf,
    sequence: u32,
    previous: String,
    poisoned: bool,
    pub epoch: String,
    pub rounds: BTreeMap<String, Round>,
}
impl Ledger {
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        let folder = root.join("blind-review");
        // Protect the directory before any sensitive bytes are written.
        #[cfg(windows)]
        crate::windows_worker_account::private_directory(
            &folder,
            &crate::ipc::current_sid().map_err(|e| e.to_string())?,
        )?;
        #[cfg(not(windows))]
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let epoch = digest(&(
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            std::process::id(),
            root,
        ))?;
        let mut this = Self {
            folder,
            sequence: 0,
            previous: "genesis".into(),
            poisoned: false,
            epoch,
            rounds: BTreeMap::new(),
        };
        let mut paths = fs::read_dir(&this.folder)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.sort();
        require(paths.len() <= 4096, "盲评账本事件超限")?;
        for path in paths {
            require(
                path.file_name().and_then(|v| v.to_str())
                    == Some(&format!("{:08}.json", this.sequence + 1)),
                "盲评账本不完整，需对账，禁止自动重发",
            )?;
            crate::host::ordinary(&path, false)?;
            let mut bytes = vec![];
            fs::File::open(&path)
                .map_err(|e| e.to_string())?
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= 2 * 1024 * 1024, "盲评账本条目超限")?;
            let entry: Entry = serde_json::from_slice(&bytes).map_err(|_| "盲评账本条目损坏")?;
            require(
                entry.sequence == this.sequence + 1
                    && entry.previous == this.previous
                    && entry.hash == digest(&(entry.sequence, &entry.previous, &entry.round))?,
                "盲评账本链损坏",
            )?;
            entry.round.validate()?;
            transition(this.rounds.get(&entry.round.id), &entry.round)?;
            this.sequence = entry.sequence;
            this.previous = entry.hash;
            this.rounds.insert(entry.round.id.clone(), entry.round);
        }
        let pending = this
            .rounds
            .values()
            .filter(|r| r.phase == ReviewPhase::Collecting)
            .cloned()
            .collect::<Vec<_>>();
        for mut r in pending {
            r.phase = ReviewPhase::Incomplete;
            r.pending = None;
            r.message =
                Some("核心已重启或数据已恢复；原轮次许可已撤销，未自动重发，部分票保持密封".into());
            this.commit(r)?;
        }
        let interrupted = this
            .rounds
            .values()
            .filter(|r| {
                r.phase == ReviewPhase::Rejected
                    && matches!(r.analysis, None | Some(AnalysisState::Pending { .. }))
            })
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in interrupted {
            this.fail_analysis(
                &id,
                "核心重启或恢复，副指挥分析未完成；不会自动重发，原拒绝票保留",
            )?;
        }
        Ok(this)
    }
    fn commit(&mut self, round: Round) -> Result<(), String> {
        require(
            !self.poisoned && self.sequence < 4096,
            "盲评账本不可继续写入",
        )?;
        round.validate()?;
        transition(self.rounds.get(&round.id), &round)?;
        let sequence = self.sequence + 1;
        let hash = digest(&(sequence, &self.previous, &round))?;
        let entry = Entry {
            sequence,
            previous: self.previous.clone(),
            round,
            hash,
        };
        let data = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        require(data.len() <= 2 * 1024 * 1024, "盲评持久条目超限")?;
        self.poisoned = true;
        let pending = self.folder.join(format!("{sequence:08}.pending"));
        let dest = self.folder.join(format!("{sequence:08}.json"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(&data)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        #[cfg(test)]
        if sequence == 3
            && std::env::var_os("BLIND_REVIEW_TEST_CRASH_PENDING").is_some()
            && std::env::var_os("BLIND_REVIEW_TEST_CRASH_ROOT")
                .is_some_and(|p| self.folder == PathBuf::from(p).join("blind-review"))
        {
            std::process::exit(73);
        }
        require(!dest.exists(), "盲评条目已存在")?;
        fs::rename(pending, dest).map_err(|e| e.to_string())?;
        self.sequence = sequence;
        self.previous = entry.hash;
        self.rounds.insert(entry.round.id.clone(), entry.round);
        self.poisoned = false;
        Ok(())
    }
    pub fn latest(&self, project: &str) -> Option<&Round> {
        self.rounds
            .values()
            .filter(|r| r.binding.project_id == project)
            .max_by(|a, b| a.id.cmp(&b.id))
    }
    pub fn prepare(
        &mut self,
        binding: ReviewBinding,
        material: ReviewMaterial,
        seats: Vec<ReviewSeat>,
        coverage: ReviewCoverage,
    ) -> Result<String, String> {
        require(
            self.latest(&binding.project_id)
                .is_none_or(|r| !r.phase.active()),
            "请先结束现有盲评轮次",
        )?;
        require(
            self.rounds.values().all(|r| !r.phase.active()),
            "请先完成或取消当前盲评，首版每次处理一个项目",
        )?;
        require(self.rounds.len() < 128, "盲评历史轮次已达容量")?;
        let id = format!("review-{:08}", self.sequence + 1);
        let key = review_digest(&binding, &seats)?;
        self.commit(Round {
            id: id.clone(),
            binding,
            material,
            coverage,
            seats,
            digest: key,
            phase: ReviewPhase::Prepared,
            deadline: None,
            deputy_ready: false,
            pending: None,
            ballots: BTreeMap::new(),
            message: None,
            analysis: None,
        })?;
        Ok(id)
    }
    pub fn checked(&self, project: &str, id: &str) -> Result<Round, String> {
        let r = self.rounds.get(id).ok_or("盲评轮次不存在")?;
        require(
            r.binding.project_id == project
                && self.latest(project).map(|r| r.id.as_str()) == Some(id),
            "盲评项目或当前轮次不符",
        )?;
        Ok(r.clone())
    }
    pub fn start(&mut self, project: &str, id: &str, key: &str, now: i64) -> Result<(), String> {
        let mut r = self.checked(project, id)?;
        require(
            r.phase == ReviewPhase::Prepared && r.digest == key,
            "盲评批准已失效",
        )?;
        r.phase = ReviewPhase::Collecting;
        r.deadline = Some(now + 900);
        self.commit(r)
    }
    pub fn reserve(
        &mut self,
        id: &str,
        now: i64,
    ) -> Result<Option<(Pending, ReviewSeat, ReviewMaterial)>, String> {
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            r.phase == ReviewPhase::Collecting && r.pending.is_none(),
            "本轮不能派发或已有未决请求",
        )?;
        if r.deadline.is_none_or(|d| now >= d) {
            self.end(id, ReviewPhase::Incomplete, "盲评到期，部分票保持密封")?;
            return Ok(None);
        }
        let readiness = !r.deputy_ready;
        let seat = if readiness {
            r.seats.iter().find(|s| s.role == "deputy")
        } else {
            r.seats.iter().find(|s| !r.ballots.contains_key(&s.id))
        }
        .ok_or("盲评派发席位缺失")?
        .clone();
        let pending = Pending {
            epoch: self.epoch.clone(),
            request: format!("request-{:08}", self.sequence + 1),
            seat: seat.id.clone(),
            readiness,
        };
        r.pending = Some(pending.clone());
        let material = r.material.clone();
        self.commit(r)?;
        Ok(Some((pending, seat, material)))
    }
    pub fn complete(
        &mut self,
        id: &str,
        pending: &Pending,
        model: &str,
        ballot: Ballot,
        now: i64,
    ) -> Result<(), String> {
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            r.phase == ReviewPhase::Collecting
                && r.pending.as_ref() == Some(pending)
                && pending.epoch == self.epoch
                && r.deadline.is_some_and(|d| now < d),
            "过期、重复或异轮次评审回包被拒绝",
        )?;
        let seat = r
            .seats
            .iter()
            .find(|s| s.id == pending.seat)
            .ok_or("评审席位缺失")?;
        require(
            seat.allowed_models.iter().any(|m| m == model),
            "实际评审模型不符合批准规则",
        )?;
        ballot.validate(&r.material)?;
        #[cfg(windows)]
        crate::independent_verifier::check_ballot(&r.material, seat, model, &ballot)?;
        r.pending = None;
        if pending.readiness {
            require(
                seat.role == "deputy" && !r.deputy_ready && r.ballots.is_empty(),
                "副指挥就绪确认重复",
            )?;
            if ballot.vote == Vote::Pass {
                r.deputy_ready = true;
            } else {
                r.phase = ReviewPhase::Incomplete;
                r.message = Some("副指挥未确认具备整体验收条件；需核对材料与方案".into());
            }
        } else {
            require(
                r.deputy_ready && !r.ballots.contains_key(&seat.id),
                "评审票重复或副指挥尚未确认",
            )?;
            r.ballots.insert(
                seat.id.clone(),
                RevealedBallot {
                    seat: seat.id.clone(),
                    model: model.into(),
                    ballot,
                },
            );
            if r.ballots.len() == r.seats.len() {
                #[cfg(windows)]
                let passed = crate::independent_verifier::tally(
                    &r.material,
                    &r.seats,
                    &r.ballots.values().cloned().collect::<Vec<_>>(),
                )?;
                #[cfg(not(windows))]
                let passed = r.ballots.values().all(|b| b.ballot.vote == Vote::Pass);
                r.phase = if passed {
                    ReviewPhase::AwaitingUser
                } else {
                    ReviewPhase::Rejected
                };
            }
        }
        self.commit(r)
    }
    pub fn end(&mut self, id: &str, phase: ReviewPhase, message: &str) -> Result<(), String> {
        require(
            matches!(
                phase,
                ReviewPhase::Incomplete
                    | ReviewPhase::Cancelled
                    | ReviewPhase::Superseded
                    | ReviewPhase::ChangesRequested
            ),
            "盲评终态无效",
        )?;
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            !matches!(r.analysis, Some(AnalysisState::Pending { .. })),
            "副指挥分析尚未结束，请保留原轮次",
        )?;
        require(
            r.phase.active()
                || (phase == ReviewPhase::ChangesRequested
                    && matches!(r.phase, ReviewPhase::Rejected | ReviewPhase::Accepted)),
            "盲评轮次已经结束",
        )?;
        require(message.len() <= 4096, "盲评说明超限")?;
        r.phase = phase;
        r.pending = None;
        r.message = Some(message.into());
        self.commit(r)
    }
    pub fn reserve_analysis(
        &mut self,
        id: &str,
        now: i64,
    ) -> Result<(Pending, ReviewSeat, ReviewMaterial, Vec<RevealedBallot>), String> {
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            r.phase == ReviewPhase::Rejected
                && r.analysis.is_none()
                && r.deadline.is_some_and(|d| now < d),
            "原轮次不能派发副指挥分析",
        )?;
        let seat = r
            .seats
            .iter()
            .find(|s| s.role == "deputy")
            .ok_or("副指挥席位缺失")?
            .clone();
        let p = Pending {
            epoch: self.epoch.clone(),
            request: format!("analysis-{:08}", self.sequence + 1),
            seat: seat.id.clone(),
            readiness: false,
        };
        let ballots = r.ballots.values().cloned().collect();
        let material = r.material.clone();
        r.analysis = Some(AnalysisState::Pending { request: p.clone() });
        self.commit(r)?;
        Ok((p, seat, material, ballots))
    }
    pub fn complete_analysis(
        &mut self,
        id: &str,
        pending: &Pending,
        model: &str,
        analysis: FailureAnalysis,
        now: i64,
    ) -> Result<(), String> {
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            r.phase == ReviewPhase::Rejected
                && pending.epoch == self.epoch
                && r.analysis
                    == Some(AnalysisState::Pending {
                        request: pending.clone(),
                    })
                && r.deadline.is_some_and(|d| now < d),
            "副指挥分析回包已过期或不符",
        )?;
        analysis.validate()?;
        r.analysis = Some(AnalysisState::Completed {
            model: model.into(),
            analysis,
        });
        self.commit(r)
    }
    pub fn fail_analysis(&mut self, id: &str, message: &str) -> Result<(), String> {
        let mut r = self.rounds.get(id).ok_or("盲评轮次不存在")?.clone();
        require(
            r.phase == ReviewPhase::Rejected
                && matches!(r.analysis, None | Some(AnalysisState::Pending { .. })),
            "原分析已结束",
        )?;
        r.analysis = Some(AnalysisState::Failed {
            message: message.into(),
        });
        self.commit(r)
    }
    pub fn confirm(
        &mut self,
        project: &str,
        id: &str,
        key: &str,
        current: &ReviewBinding,
    ) -> Result<(), String> {
        let mut r = self.checked(project, id)?;
        require(
            r.phase == ReviewPhase::AwaitingUser && r.digest == key && r.binding == *current,
            "盲评材料或状态已变化，不能确认旧结果",
        )?;
        #[cfg(windows)]
        require(
            crate::independent_verifier::tally(
                &r.material,
                &r.seats,
                &r.ballots.values().cloned().collect::<Vec<_>>(),
            )?,
            "独立验收未确认全票通过",
        )?;
        r.phase = ReviewPhase::Accepted;
        self.commit(r)
    }
}
fn transition(old: Option<&Round>, new: &Round) -> Result<(), String> {
    let Some(old) = old else {
        return require(
            new.phase == ReviewPhase::Prepared
                && new.ballots.is_empty()
                && !new.deputy_ready
                && new.pending.is_none()
                && new.deadline.is_none()
                && new.analysis.is_none(),
            "盲评初始状态必须先准备",
        );
    };
    require(
        old.id == new.id
            && old.binding == new.binding
            && old.material == new.material
            && old.coverage == new.coverage
            && old.seats == new.seats
            && old.digest == new.digest,
        "盲评轮次内不能替换材料、名册或政策",
    )?;
    require(
        match (&old.analysis, &new.analysis) {
            (None, _) => true,
            (
                Some(AnalysisState::Pending { .. }),
                Some(AnalysisState::Completed { .. } | AnalysisState::Failed { .. }),
            ) => true,
            (a, b) => a == b,
        },
        "副指挥分析不可重发或覆盖",
    )?;
    require(
        old.ballots
            .iter()
            .all(|(id, v)| new.ballots.get(id) == Some(v))
            && (!old.deputy_ready || new.deputy_ready)
            && (old.deadline.is_none() || old.deadline == new.deadline),
        "盲评已封票、就绪或期限不能回滚",
    )?;
    let allowed = match old.phase {
        ReviewPhase::Prepared => matches!(
            new.phase,
            ReviewPhase::Collecting
                | ReviewPhase::Cancelled
                | ReviewPhase::Incomplete
                | ReviewPhase::Superseded
                | ReviewPhase::ChangesRequested
        ),
        ReviewPhase::Collecting => matches!(
            new.phase,
            ReviewPhase::Collecting
                | ReviewPhase::AwaitingUser
                | ReviewPhase::Rejected
                | ReviewPhase::Incomplete
                | ReviewPhase::Cancelled
                | ReviewPhase::Superseded
                | ReviewPhase::ChangesRequested
        ),
        ReviewPhase::AwaitingUser => matches!(
            new.phase,
            ReviewPhase::Accepted
                | ReviewPhase::Cancelled
                | ReviewPhase::Incomplete
                | ReviewPhase::Superseded
                | ReviewPhase::ChangesRequested
        ),
        ReviewPhase::Rejected => {
            new.phase == ReviewPhase::ChangesRequested
                || (new.phase == ReviewPhase::Rejected && old.analysis != new.analysis)
        }
        ReviewPhase::Accepted => new.phase == ReviewPhase::ChangesRequested,
        _ => false,
    };
    require(allowed, "盲评终态或事件顺序不允许此转换")
}
