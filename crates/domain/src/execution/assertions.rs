//! Explicit checker assertions. Exit status alone cannot establish test coverage.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssertionResult {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssertionReport {
    pub schema_version: u32,
    pub assertions: Vec<AssertionResult>,
}

pub fn validate_assertion_ids(ids: &[String]) -> Result<(), String> {
    require(ids.len() <= 64, "at most 64 expected assertions per check")?;
    let mut seen = BTreeSet::new();
    for id in ids {
        identifier(id)?;
        require(seen.insert(id), "duplicate expected assertion")?;
    }
    Ok(())
}

/// Parse only the declared protocol; legacy checks retain their original bytes.
pub fn parse_assertions(
    check: &AcceptanceCheck,
    stdout: &[u8],
) -> Result<Vec<AssertionResult>, String> {
    if check.expected_assertions.is_empty() {
        return Ok(vec![]);
    }
    validate_assertion_ids(&check.expected_assertions)?;
    let report: AssertionReport = serde_json::from_slice(stdout)
        .map_err(|_| "checker must return a schemaVersion=1 JSON assertion report")?;
    require(report.schema_version == 1, "unsupported assertion report")?;
    require_assertions(check, &report.assertions, false)?;
    Ok(report.assertions)
}

pub fn require_assertions(
    check: &AcceptanceCheck,
    results: &[AssertionResult],
    passed: bool,
) -> Result<(), String> {
    let required: BTreeSet<_> = check.expected_assertions.iter().collect();
    let observed: BTreeSet<_> = results.iter().map(|r| &r.id).collect();
    require(
        required == observed && observed.len() == results.len(),
        "assertions missing, duplicated or unexpected",
    )?;
    for result in results {
        identifier(&result.id)?;
        require(
            !result.detail.trim().is_empty()
                && result.detail.len() <= 2048
                && !result.detail.contains('\0'),
            "assertion evidence detail required",
        )?;
        require(!passed || result.passed, "required assertion failed")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check() -> AcceptanceCheck {
        AcceptanceCheck {
            expected_assertions: vec!["normal".into(), "invalid-input".into()],
            id: "check".into(),
            name: None,
            program: "checker.exe".into(),
            args: vec![],
            timeout_seconds: 10,
            tool: None,
        }
    }
    #[test]
    fn zero_exit_without_assertions_is_not_coverage() {
        for bytes in [
            b"success".as_slice(),
            br#"{"schemaVersion":1,"assertions":[]}"#,
        ] {
            assert!(parse_assertions(&check(), bytes).is_err());
        }
    }
    #[test]
    fn missing_duplicate_failed_or_wrong_assertions_cannot_pass() {
        let check = check();
        let good = vec![
            AssertionResult {
                id: "normal".into(),
                passed: true,
                detail: "1+2=3".into(),
            },
            AssertionResult {
                id: "invalid-input".into(),
                passed: true,
                detail: "rejects text".into(),
            },
        ];
        require_assertions(&check, &good, true).unwrap();
        assert!(require_assertions(&check, &good[..1], true).is_err());
        let mut bad = good.clone();
        bad[1].id = "normal".into();
        assert!(require_assertions(&check, &bad, true).is_err());
        let mut bad = good;
        bad[1].passed = false;
        assert!(require_assertions(&check, &bad, true).is_err());
        require_assertions(&check, &bad, false).unwrap();
    }
}
