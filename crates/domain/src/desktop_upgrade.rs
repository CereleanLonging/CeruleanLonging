//! Compatibility eligibility and evidence for Qoder's observed 0.4 UI protocol.
use crate::desktop_sessions::{DesktopApp, DesktopBinding, DesktopTrial};

pub fn qoder_candidate(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 4
        && parts[0] == "0"
        && parts[1] == "4"
        && parts[2..].iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
        && parts[2].parse::<u16>().is_ok_and(|patch| patch >= 3)
}

pub fn qoder_established(version: &str) -> bool {
    matches!(version, "0.4.3.0" | "0.4.8.0")
}

/// A probe permits only the observed installation/session/model/bridge tuple.
/// New work still requires a fresh saved binding, plan approval and isolation.
pub fn require_upgrade_trial(
    binding: &DesktopBinding,
    trials: &[DesktopTrial],
    bridge: &str,
) -> Result<(), String> {
    let observation = &binding.observation;
    crate::execution::require(
        observation.window.app == DesktopApp::Qoder && qoder_candidate(&observation.window.version),
        "当前桌面版本超出 Qoder 0.4 接口适配范围，请等待对应适配器更新",
    )?;
    if qoder_established(&observation.window.version) {
        return Ok(());
    }
    let rebound = chrono::DateTime::parse_from_rfc3339(&observation.captured_at)
        .map_err(|_| "升级后会话观察时间无效")?;
    let passed = trials.iter().any(|trial| {
        let Some(proof) = &trial.proof else {
            return false;
        };
        let previous = &trial.review.observation;
        binding.bridge_sha256 == bridge
            && trial.review.bridge_sha256 == bridge
            && proof.reader_sha256.as_deref() == Some(bridge)
            && previous.window == observation.window
            && previous.session_id == observation.session_id
            && previous.model_label == observation.model_label
            && proof.marker == trial.review.marker
            && Some(&proof.session_id) == observation.session_id.as_ref()
            && proof.baseline == trial.review.baseline
            && proof.prompt_seen
            && proof.completed
            && proof.reply_matches
            && chrono::DateTime::parse_from_rfc3339(&proof.observed_at)
                .is_ok_and(|completed| completed <= rebound)
    });
    crate::execution::require(
        passed,
        "Qoder 升级版本需要先完成当前聊天的固定消息验证，再重新读取并保存关联、批准方案",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_sessions::*;

    #[test]
    fn only_observed_protocol_family_can_be_probed() {
        for version in [
            "0.4.3.0",
            "0.4.8.0",
            "0.4.8.1",
            "0.4.9.0",
            "0.4.65535.65535",
        ] {
            assert!(qoder_candidate(version), "{version}");
        }
        for version in [
            "",
            "unknown",
            "0.4.2.0",
            "0.5.0.0",
            "1.4.9.0",
            "0.4.9",
            "0.4.9.0-beta",
            "0.4.+9.0",
            "0.4.9.65536",
        ] {
            assert!(!qoder_candidate(version), "{version}");
        }
        assert!(qoder_established("0.4.8.0"));
        assert!(!qoder_established("0.4.9.0"));
    }

    fn fixture() -> (DesktopBinding, DesktopTrial) {
        let observation = DesktopObservation {
            window: DesktopWindow {
                app: DesktopApp::Qoder,
                hwnd: "100".into(),
                pid: 10,
                start_ticks: "123".into(),
                exe: "D:/Qoder/Qoder.exe".into(),
                title: "project".into(),
                version: "0.4.9.0".into(),
            },
            captured_at: "2026-11-07T00:00:00Z".into(),
            document_runtime_id: "1.2".into(),
            session_id: Some("session".into()),
            model_label: Some("model".into()),
            content_digest: "c".repeat(64),
            layout_digest: "d".repeat(64),
            node_count: 30,
            draft_empty: true,
            busy: false,
        };
        let trial = DesktopTrial {
            review: DesktopTrialReview {
                id: "a".repeat(64),
                binding_id: "b".repeat(64),
                observation: observation.clone(),
                marker: "marker".into(),
                prompt: "probe".into(),
                baseline: vec!["1".repeat(64), "2".repeat(64)],
                bridge_sha256: "f".repeat(64),
            },
            started_at: "2026-11-07T00:00:01Z".into(),
            proof: Some(DesktopTrialProof {
                observed_baseline: None,
                reader_sha256: Some("f".repeat(64)),
                marker: "marker".into(),
                session_id: "session".into(),
                baseline: vec!["1".repeat(64), "2".repeat(64)],
                prompt_seen: true,
                completed: true,
                reply_matches: true,
                observed_at: "2026-11-07T00:00:02Z".into(),
            }),
        };
        let mut binding = DesktopBinding {
            id: "e".repeat(64),
            project_id: "project".into(),
            label: "upgraded".into(),
            observation,
            confirmed_at: "2026-11-07T00:00:04Z".into(),
            bridge_sha256: "f".repeat(64),
        };
        binding.observation.captured_at = "2026-11-07T00:00:03Z".into();
        (binding, trial)
    }

    #[test]
    fn upgrade_requires_matching_completed_probe_and_new_observation() {
        let (binding, trial) = fixture();
        let bridge = "f".repeat(64);
        assert!(require_upgrade_trial(&binding, &[], &bridge).is_err());
        assert!(require_upgrade_trial(&binding, &[trial.clone()], &bridge).is_ok());
        for field in 0..13 {
            let mut changed = binding.clone();
            let mut probe = trial.clone();
            match field {
                0 => changed.observation.window.version = "0.4.10.0".into(),
                1 => changed.observation.window.pid += 1,
                2 => changed.observation.window.start_ticks.push('1'),
                3 => changed.observation.window.exe = "D:/other/Qoder.exe".into(),
                4 => changed.observation.session_id = Some("other".into()),
                5 => changed.observation.model_label = Some("other".into()),
                6 => changed.bridge_sha256 = "0".repeat(64),
                7 => probe.proof.as_mut().unwrap().reader_sha256 = None,
                8 => probe.proof.as_mut().unwrap().completed = false,
                9 => probe.proof = None,
                10 => changed.observation.captured_at = "2026-11-07T00:00:01Z".into(),
                11 => probe.proof.as_mut().unwrap().reply_matches = false,
                12 => probe.review.bridge_sha256 = "0".repeat(64),
                _ => unreachable!(),
            }
            assert!(
                require_upgrade_trial(&changed, &[probe], &bridge).is_err(),
                "{field}"
            );
        }
    }
}
