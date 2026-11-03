//! Explicit, backup-bound reader promotion. Container schema remains independent.
use crate::data_format::Status;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub const CURRENT: u32 = 3;
pub const MARKER: &str = "events/00000000.reader3.json";
pub const FILE_NAME: &str = "00000000.reader3.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Guard {
    schema: u32,
    product: String,
    minimum_reader_generation: u32,
    data_schema: u32,
    source_marker_digest: Option<String>,
    source_backup_digest: String,
    root: String,
    owner_sid: String,
    promoted_at: String,
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    let mut file = crate::backup::open(path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Reader preflight record exceeds limit".into());
    }
    Ok(bytes)
}
pub(crate) fn observe(root: &Path, status: &mut Status) -> Result<(), String> {
    observe_legacy(root, status)?;
    crate::currency_guard::observe(root, status)?;
    crate::currency_execution_guard::observe(root, status)?;
    crate::currency_count_guard::observe(root, status)?;
    crate::currency_count_resolution_guard::observe(root, status)?;
    crate::currency_scoped_guard::observe(root, status)?;
    crate::currency_scoped_count_guard::observe(root, status)?;
    crate::currency_scoped_execution_guard::observe(root, status)?;
    crate::currency_api_guard::observe(root, status)?;
    crate::currency_api_resolution_guard::observe(root, status)?;
    crate::management_source_guard::observe(root, status)?;
    crate::experiment_guard::observe(root, status)?;
    crate::relay_guard::observe(root, status)?;
    crate::rejected_usage_guard::observe(root, status)?;
    crate::verification_recheck_guard::observe(root, status)?;
    crate::blind_review_guard::observe(root, status)?;
    crate::worker_account_guard::observe(root, status)
}
fn observe_legacy(root: &Path, status: &mut Status) -> Result<(), String> {
    match fs::read_dir(root.join("events")) {
        Ok(entries) => {
            for entry in entries {
                let name = entry.map_err(|e| e.to_string())?.file_name();
                if name
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("00000000.reader")
                    && name != FILE_NAME
                    && name != crate::currency_guard::FILE_NAME
                    && name != crate::currency_execution_guard::FILE_NAME
                    && name != crate::currency_count_guard::FILE_NAME
                    && name != crate::currency_count_resolution_guard::FILE_NAME
                    && name != crate::currency_scoped_guard::FILE_NAME
                    && name != crate::currency_scoped_count_guard::FILE_NAME
                    && name != crate::currency_scoped_execution_guard::FILE_NAME
                    && name != crate::currency_api_guard::FILE_NAME
                    && name != crate::currency_api_resolution_guard::FILE_NAME
                    && name != crate::management_source_guard::FILE_NAME
                    && name != crate::relay_guard::FILE_NAME
                    && name != crate::rejected_usage_guard::FILE_NAME
                    && name != crate::verification_recheck_guard::FILE_NAME
                    && name != crate::blind_review_guard::FILE_NAME
                    && name != crate::worker_account_guard::FILE_NAME
                    && name != crate::experiment_guard::FILE_NAME
                    && name != crate::experiment_guard::LEGACY_FILE_NAME
                {
                    return Err("Unsupported reader guard filename; no data loaded".into());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let path = root.join(MARKER);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    #[cfg(windows)]
    let _pins = [root.to_path_buf(), root.join("events")]
        .iter()
        .map(|p| crate::backup::open(p, true, false).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = read(&path, 65536)?;
    let guard: Guard =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid reader guard; no data loaded")?;
    if guard.schema != 1
        || guard.product != cerulean_domain::PRODUCT_NAME
        || guard.minimum_reader_generation != CURRENT
        || guard.data_schema != status.data_schema
        || guard.source_marker_digest != status.marker_digest
        || !hex(&guard.source_backup_digest)
        || Path::new(&guard.root) != fs::canonicalize(root).map_err(|e| e.to_string())?
        || chrono::DateTime::parse_from_rfc3339(&guard.promoted_at).is_err()
    {
        return Err(
            "Unsupported reader generation or changed original container; no data loaded".into(),
        );
    }
    #[cfg(windows)]
    if guard.owner_sid != crate::ipc::current_sid().map_err(|e| e.to_string())? {
        return Err("Reader guard belongs to another Windows user".into());
    }
    status.minimum_reader_generation = CURRENT;
    status.reader_guard_digest = Some(format!("{:x}", Sha256::digest(bytes)));
    Ok(())
}

pub(crate) fn require_override(root: &Path, present: bool) -> Result<(), String> {
    if present && crate::data_format::status(root)?.minimum_reader_generation < CURRENT {
        return Err("supervisionOverride requires reader generation 3: stop the core, hold and back up the original data, then explicitly review and promote the reader before writing".into());
    }
    Ok(())
}
pub(crate) fn require_revision(
    root: &Path,
    revision: &cerulean_domain::execution::PlanRevision,
) -> Result<(), String> {
    crate::relay_guard::require_protected(root, crate::relay_guard::revision_uses_relay(revision))?;
    crate::currency_guard::require_protected(root, revision.policy.currency_budget.is_some())?;
    crate::currency_count_guard::require_settings(root, revision.policy.currency_budget.as_ref())?;
    crate::currency_scoped_guard::require_protected(
        root,
        revision
            .policy
            .currency_budget
            .as_ref()
            .is_some_and(|settings| settings.scoped_quotes.is_some()),
    )?;
    require_override(
        root,
        revision
            .execution
            .iter()
            .any(|e| e.supervision_override.is_some()),
    )
}
pub(crate) fn require_workflow_context(
    root: &Path,
    context: &crate::scheduler::CommandContext,
) -> Result<(), String> {
    if context.verification_recheck.is_some()
        || context.recheck_disposition.is_some()
        || matches!(
            context.execution,
            Some(cerulean_domain::execution::ExecutionEvidence::VerificationRecheck { .. })
        )
    {
        let data = root
            .parent()
            .filter(|p| p.file_name().is_some_and(|v| v == "workflows"))
            .and_then(Path::parent)
            .ok_or("recheck evidence requires its guarded Host directory")?;
        crate::verification_recheck_guard::require_protected(data, true)?;
    }
    if context.currency_approval.is_some()
        || context.planning_budget.is_some()
        || context.historical_charge.is_some()
        || context.count_resolution.is_some()
    {
        let data = root
            .parent()
            .filter(|p| p.file_name().is_some_and(|v| v == "workflows"))
            .and_then(Path::parent)
            .ok_or("Currency approvals require a guarded host directory")?;
        crate::currency_guard::require_protected(data, true)?;
        crate::currency_count_resolution_guard::require_protected(
            data,
            context.count_resolution.is_some(),
        )?;
        if let Some(planning) = &context.planning_budget {
            crate::currency_count_guard::require_quotes(data, &planning.policy.quotes)?;
            crate::currency_scoped_guard::require_protected(
                data,
                planning.policy.scoped_quotes.is_some(),
            )?;
        }
    }
    if let Some(cerulean_domain::execution::ExecutionEvidence::Revision { revision }) =
        &context.execution
    {
        if crate::relay_guard::revision_uses_relay(revision)
            || revision.policy.currency_budget.is_some()
            || revision
                .execution
                .iter()
                .any(|e| e.supervision_override.is_some())
        {
            let data = root
                .parent()
                .filter(|p| p.file_name().is_some_and(|v| v == "workflows"))
                .and_then(Path::parent)
                .ok_or("Reader-3 revisions require a guarded host data directory")?;
            require_revision(data, revision)?;
        }
    }
    Ok(())
}

// Inspect only protocol-owned fields, never arbitrary task text or business JSON.
// This preflight does not repair records or create directories/journals.
pub(crate) fn reject_unprotected(root: &Path) -> Result<(), String> {
    crate::verification_recheck_guard::reject_unprotected(root)?;
    crate::rejected_usage_guard::reject_unprotected(root)?;
    crate::experiment_guard::reject_unprotected(root)?;
    reject_legacy_unprotected(root)
}
pub(crate) fn reject_legacy_unprotected(root: &Path) -> Result<(), String> {
    let generation = crate::data_format::status(root)?.minimum_reader_generation;
    if generation >= crate::relay_guard::GENERATION {
        return Ok(());
    }
    if generation < crate::currency_guard::GENERATION
        && root
            .join("currency-budgets")
            .try_exists()
            .map_err(|e| e.to_string())?
    {
        return Err(
            "Existing currency ledger has no reader4 guard; preserve the original data".into(),
        );
    }
    let mut paths = Vec::new();
    collect_json(&root.join("project-setups"), &mut paths)?;
    collect_json(&root.join("management-decisions"), &mut paths)?;
    if root
        .join("currency-budgets")
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        ordinary_dir(&root.join("currency-budgets"))?;
        for entry in fs::read_dir(root.join("currency-budgets")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            collect_json(&path, &mut paths)?;
        }
    }
    let workflows = root.join("workflows");
    if workflows.try_exists().map_err(|e| e.to_string())? {
        ordinary_dir(&workflows)?;
        for entry in fs::read_dir(&workflows).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            ordinary_dir(&entry.path())?;
            collect_json(&entry.path().join("events"), &mut paths)?;
        }
    }
    // The original API journal lives under each workflow's attempt/rounds path.
    if generation < crate::currency_api_guard::GENERATION {
        collect_api_rounds(&workflows, &mut paths)?;
    }
    for path in paths {
        let parsed = serde_json::from_slice::<serde_json::Value>(&read(&path, 64 * 1024 * 1024)?);
        // Original management fragment review owns malformed pending records.
        // Parseable scoped records still require the new reader before recovery.
        let management_pending = path.extension().is_some_and(|s| s == "pending")
            && path
                .parent()
                .is_some_and(|p| p.file_name().is_some_and(|n| n == "management-decisions"));
        let value = match parsed {
            Ok(value) => value,
            Err(_) if management_pending => continue,
            Err(_) => return Err("Invalid record during reader preflight".into()),
        };
        if generation < crate::relay_guard::GENERATION
            && crate::relay_guard::record_uses_relay(&value)
        {
            return Err(
                "Existing A6API records require reader16 protection; preserve original data".into(),
            );
        }
        if generation < crate::management_source_guard::GENERATION
            && path
                .parent()
                .is_some_and(|p| p.file_name().is_some_and(|n| n == "management-decisions"))
            && (value
                .pointer("/body/version")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v >= 2)
                || value
                    .pointer("/body/evidence/kind")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| matches!(s, "externalIntent" | "externalResponse")))
        {
            return Err("External management evidence requires reader13 protection; retain original data, do not auto-promote".into());
        }
        if generation < crate::currency_api_resolution_guard::GENERATION
            && (value
                .pointer("/body/context/apiCountResolution")
                .is_some_and(|v| !v.is_null())
                || value
                    .pointer("/body/context/apiGenerationResolution")
                    .is_some_and(|v| !v.is_null())
                || matches!(
                    value.pointer("/body/event/kind").and_then(|v| v.as_str()),
                    Some(
                        "resolveApiCount"
                            | "ResolveApiCount"
                            | "resolveApiGeneration"
                            | "ResolveApiGeneration"
                    )
                )
                || matches!(
                    value
                        .pointer("/body/event/command/kind")
                        .and_then(|v| v.as_str()),
                    Some(
                        "confirm-api-generation-resolution"
                            | "confirmApiGenerationResolution"
                            | "ConfirmApiGenerationResolution"
                            | "confirm-api-count-resolution"
                            | "confirmApiCountResolution"
                            | "ConfirmApiCountResolution"
                    )
                ))
        {
            return Err(
                "Existing API fee resolution has no reader12 guard; preserve original data".into(),
            );
        }
        if generation < crate::currency_api_guard::GENERATION
            && (matches!(
                value.pointer("/body/event/kind").and_then(|v| v.as_str()),
                Some(
                    "beginApiCount"
                        | "recordApiCount"
                        | "apiCountUnknown"
                        | "scopedIntent"
                        | "scopedOutcome"
                        | "feeAck"
                )
            ) || value
                .pointer("/body/event/scopedApiPreparation")
                .is_some_and(|v| !v.is_null()))
        {
            return Err(
                "Existing API monetary evidence has no reader11 guard; preserve original data"
                    .into(),
            );
        }
        if generation < crate::currency_scoped_execution_guard::GENERATION
            && ([
                "/body/event/scopedPreparation",
                "/body/evidence/scoped",
                "/body/evidence/currencyAck",
            ]
            .iter()
            .any(|p| value.pointer(p).is_some_and(|v| !v.is_null()))
                || value
                    .pointer("/body/evidence/kind")
                    .and_then(|v| v.as_str())
                    == Some("scopedResponse"))
        {
            return Err(
                "Existing scoped execution has no reader10 guard; preserve original data".into(),
            );
        }
        if generation < crate::currency_scoped_count_guard::GENERATION
            && (value
                .pointer("/body/event/attempt/schema")
                .and_then(|v| v.as_u64())
                == Some(2)
                || value
                    .pointer("/body/event/attempt/identity/request/requestedService")
                    .is_some_and(|v| !v.is_null())
                || matches!(
                    value.pointer("/body/event/kind").and_then(|v| v.as_str()),
                    Some("recordScopedManagementCount" | "RecordScopedManagementCount")
                ))
        {
            return Err(
                "Existing scoped count has no reader9 guard; preserve original data".into(),
            );
        }
        if generation < crate::currency_count_resolution_guard::GENERATION
            && (value
                .pointer("/body/context/countResolution")
                .is_some_and(|v| !v.is_null())
                || value
                    .pointer("/body/event/command/kind")
                    .and_then(|v| v.as_str())
                    .is_some_and(|kind| {
                        matches!(
                            kind,
                            "confirm-management-count-resolution"
                                | "ConfirmManagementCountResolution"
                                | "confirmManagementCountResolution"
                        )
                    })
                || value
                    .pointer("/body/event/kind")
                    .and_then(|v| v.as_str())
                    .is_some_and(|kind| {
                        matches!(kind, "ResolveManagementCount" | "resolveManagementCount")
                    }))
        {
            return Err(
                "Existing count resolution has no reader7 guard; preserve original data".into(),
            );
        }
        for pointer in [
            "/setup/policy/currencyBudget",
            "/body/context/planningBudget/policy",
            "/body/context/execution/revision/policy/currencyBudget",
            "/body/evidence/target/context/execution/revision/policy/currencyBudget",
            "/body/evidence/intent/context/executionSetup/setup/policy/currencyBudget",
            "/body/evidence/intent/context/execution/revision/policy/currencyBudget",
            "/body/evidence/intent/context/workflow/revision/policy/currencyBudget",
            "/body/event/policy",
            "/body/event/authorization/policy",
            "/body/event/preparation/intent/context/executionSetup/setup/policy/currencyBudget",
            "/body/event/preparation/intent/context/workflow/revision/policy/currencyBudget",
            "/body/event/attempt/intent/context/executionSetup/setup/policy/currencyBudget",
            "/body/event/attempt/intent/context/workflow/revision/policy/currencyBudget",
        ] {
            if generation < crate::currency_scoped_guard::GENERATION
                && value.pointer(pointer).is_some_and(|policy| {
                    policy.get("schema").and_then(|v| v.as_u64()) == Some(2)
                        || policy.get("scopedQuotes").is_some_and(|v| !v.is_null())
                })
            {
                return Err(
                    "Existing scoped pricing has no reader8 guard; preserve original data".into(),
                );
            }
        }
        for pointer in [
            "/body/event/reservation/pricing",
            "/body/event/preparation/reservation/pricing",
            "/body/event/settlement/actualService",
        ] {
            if generation < crate::currency_scoped_guard::GENERATION
                && value.pointer(pointer).is_some_and(|v| !v.is_null())
            {
                return Err(
                    "Existing scoped pricing has no reader8 guard; preserve original data".into(),
                );
            }
        }
        for pointer in [
            "/setup/policy/currencyBudget/quotes",
            "/body/context/planningBudget/policy/quotes",
            "/body/context/execution/revision/policy/currencyBudget/quotes",
            "/body/evidence/target/context/execution/revision/policy/currencyBudget/quotes",
            "/body/evidence/intent/context/executionSetup/setup/policy/currencyBudget/quotes",
            "/body/event/policy/quotes",
            "/body/event/authorization/policy/quotes",
        ] {
            if generation < crate::currency_count_guard::GENERATION
                && value
                    .pointer(pointer)
                    .and_then(|v| v.as_array())
                    .is_some_and(|quotes| {
                        quotes
                            .iter()
                            .any(|q| q.get("countPrice").is_some_and(|v| !v.is_null()))
                    })
            {
                return Err(
                    "Existing count price has no reader6 guard; preserve original data".into(),
                );
            }
        }
        if generation < crate::currency_count_guard::GENERATION
            && value
                .pointer("/body/event/kind")
                .and_then(|v| v.as_str())
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "BeginManagementCount"
                            | "RecordManagementCount"
                            | "ManagementCountUnknown"
                            | "beginManagementCount"
                            | "recordManagementCount"
                            | "managementCountUnknown"
                    )
                })
        {
            return Err(
                "Existing count attempt has no reader6 guard; preserve original data".into(),
            );
        }
        for pointer in [
            "/setup/policy/currencyBudget",
            "/body/context/currencyApproval",
            "/body/context/planningBudget",
            "/body/context/historicalCharge",
            "/body/evidence/request",
            "/body/evidence/transport",
            "/body/context/execution/revision/policy/currencyBudget",
            "/body/evidence/target/context/execution/revision/policy/currencyBudget",
            "/body/evidence/intent/context/executionSetup/setup/policy/currencyBudget",
            "/body/evidence/intent/context/execution/revision/policy/currencyBudget",
        ] {
            if generation < crate::currency_guard::GENERATION
                && value.pointer(pointer).is_some_and(|v| !v.is_null())
            {
                return Err("Existing currency settings have no reader4 guard; preserve original records, do not auto-promote".into());
            }
        }
        for pointer in [
            "/setup/profiles",
            "/body/context/execution/revision/execution",
            "/body/evidence/target/context/execution/revision/execution",
        ] {
            if generation < CURRENT
                && value
                    .pointer(pointer)
                    .and_then(|v| v.as_array())
                    .is_some_and(|items| {
                        items
                            .iter()
                            .any(|v| v.get("supervisionOverride").is_some_and(|v| !v.is_null()))
                    })
            {
                return Err("Existing supervisionOverride has no reader guard; preserve the original records for review, do not auto-promote".into());
            }
        }
    }
    Ok(())
}
fn ordinary_dir(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::backup::open(path, true, false).map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("Reader preflight requires ordinary directories".into());
    }
    Ok(())
}
fn collect_json(dir: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !dir.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    ordinary_dir(dir)?;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        // Currency pending records can be promoted/recovered later. Inspect
        // their protocol before any Host workflow recovery can write as well.
        if path.extension().is_some_and(|v| v == "json")
            || (dir.file_name().is_some_and(|n| n == "management-decisions")
                && path.extension().is_some_and(|v| v == "pending"))
            || (dir
                .parent()
                .is_some_and(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
                && path.extension().is_some_and(|v| v == "pending"))
        {
            paths.push(path);
        }
        if paths.len() > 100_000 {
            return Err("Reader preflight history exceeds limit".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionReview {
    pub digest: String,
    source_root: String,
    backup: String,
    backup_digest: String,
    hold_digest: String,
    data_schema: u32,
    source_marker_digest: Option<String>,
    from_reader_generation: u32,
    to_reader_generation: u32,
    migrator_version: &'static str,
    original_events_rewritten: bool,
    hold_released: bool,
}
#[cfg(windows)]
fn reviewed(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<PromotionReview, String> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    reject_unprotected(root)?;
    let status = crate::data_format::status(root)?;
    if !hex(manifest) || !hex(hold) || status.minimum_reader_generation >= CURRENT {
        return Err(
            "Reader promotion requires an unpromoted original and explicit backup/hold digests"
                .into(),
        );
    }
    let mut review = PromotionReview {
        digest: String::new(),
        source_root: root.to_string_lossy().into_owned(),
        backup: fs::canonicalize(archive)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned(),
        backup_digest: manifest.into(),
        hold_digest: hold.into(),
        data_schema: status.data_schema,
        source_marker_digest: status.marker_digest,
        from_reader_generation: status.minimum_reader_generation,
        to_reader_generation: CURRENT,
        migrator_version: env!("CARGO_PKG_VERSION"),
        original_events_rewritten: false,
        hold_released: false,
    };
    review.digest = cerulean_domain::execution::digest(&review)?;
    Ok(review)
}
#[cfg(windows)]
pub fn review(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<PromotionReview, String> {
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        reviewed(root, archive, manifest, hold)
    })
}
#[cfg(windows)]
pub fn promote(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
    expected: &str,
) -> Result<Status, String> {
    use std::io::Write;
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        let review = reviewed(root, archive, manifest, hold)?;
        if review.digest != expected {
            return Err("Reader promotion review changed; no write performed".into());
        }
        let guard = Guard {
            schema: 1,
            product: cerulean_domain::PRODUCT_NAME.into(),
            minimum_reader_generation: CURRENT,
            data_schema: review.data_schema,
            source_marker_digest: review.source_marker_digest,
            source_backup_digest: manifest.into(),
            root: root.to_string_lossy().into_owned(),
            owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
            promoted_at: chrono::Utc::now().to_rfc3339(),
        };
        let events = root.join("events");
        let _pin = crate::backup::open(&events, true, false).map_err(|e| e.to_string())?;
        let pending = events.join(format!(
            ".format-migration-reader-{}-{}.pending",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        #[cfg(test)]
        tests::fault_checkpoint("before-create");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::fault_checkpoint("pending-created");
        file.write_all(&serde_json::to_vec_pretty(&guard).map_err(|e| e.to_string())?)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::fault_checkpoint("pending-synced");
        drop(file);
        crate::backup::publish(&pending, &root.join(MARKER)).map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::fault_checkpoint("marker-published");
        crate::data_format::status(root)
    })
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use crate::{backup, upgrade, Store};
    // Explicit owned fault driver only. Neither hooks nor configuration readers
    // exist in production builds; the actual OS file operations are unchanged.
    pub(super) fn fault_checkpoint(phase: &str) {
        use std::io::Write;
        let Ok(config) = std::env::var("CERULEAN_READER_FAULT_CONFIG") else {
            return;
        };
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        if config["phase"].as_str() != Some(phase) {
            return;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(config["checkpoint"].as_str().unwrap())
            .unwrap();
        let receipt = serde_json::json!({"phase":phase,"pid":std::process::id(),"token":config["token"],"testOnlyCheckpoint":true});
        output
            .write_all(&serde_json::to_vec(&receipt).unwrap())
            .unwrap();
        output.sync_all().unwrap();
        drop(output);
        loop {
            if let Some(resume) = config["resume"].as_str() {
                if fs::read_to_string(resume).ok().as_deref() == config["token"].as_str() {
                    return;
                }
            }
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
    }
    #[test]
    #[ignore = "Owned external fault driver, actual OS I/O with test-only checkpoints"]
    fn reader_fault_child() {
        let config = std::env::var("CERULEAN_READER_FAULT_CONFIG").unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        let result = promote(
            Path::new(config["data"].as_str().unwrap()),
            Path::new(config["backup"].as_str().unwrap()),
            config["manifestDigest"].as_str().unwrap(),
            config["holdDigest"].as_str().unwrap(),
            config["reviewDigest"].as_str().unwrap(),
        );
        if config["kind"] == "io" {
            use std::io::Write;
            let error = result.expect_err("External OS lock must deny write/publication");
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(config["outcome"].as_str().unwrap())
                .unwrap();
            output.write_all(error.as_bytes()).unwrap();
            output.sync_all().unwrap();
        } else {
            panic!("Owned interrupt checkpoint was not reached");
        }
    }

    pub(crate) fn snapshot(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        // Commands acknowledge durable events before derived files are published.
        // Keep comparing EVERY file; wait for this root's admitted work instead
        // of filtering indexes/documents or racing their private temporary names.
        crate::derived_index::wait_for_test_projections(root);
        fn walk(dir: &Path, map: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, map);
                } else if path.file_name().is_some_and(|v| v == "writer.lock") {
                    // The running host locks its bytes; compare presence/length only.
                    map.insert(
                        path.clone(),
                        fs::metadata(path).unwrap().len().to_le_bytes().to_vec(),
                    );
                } else {
                    map.insert(path.clone(), fs::read(path).unwrap());
                }
            }
        }
        let mut map = Default::default();
        walk(root, &mut map);
        map
    }

    pub(crate) fn held(root: &Path, archive: &Path) -> (String, String) {
        let root = fs::canonicalize(root).unwrap();
        let mut review = upgrade::UpgradeReview {
            digest: String::new(),
            ready: true,
            blockers: vec![],
            snapshot_digest: "a".repeat(64),
            core_version: env!("CARGO_PKG_VERSION").into(),
            core_sha256: "b".repeat(64),
            server_pid: std::process::id(),
        };
        review.digest = cerulean_domain::execution::digest(&review).unwrap();
        let hold = upgrade::prepare(&root, review).unwrap().hold_digest;
        let manifest = backup::create_held(&root, archive, &hold)
            .unwrap()
            .manifest_digest;
        (manifest, hold)
    }
    pub(crate) fn promote_fixture(root: &Path) {
        drop(Store::open(root).unwrap());
        let archive = root.with_file_name("reader3-cold-backup");
        let (manifest, hold) = held(root, &archive);
        let reviewed = review(root, &archive, &manifest, &hold).unwrap();
        promote(root, &archive, &manifest, &hold, &reviewed.digest).unwrap();
        upgrade::release(root, &hold).unwrap();
    }
    #[test]
    #[ignore = "Creates explicit owned CLI fixtures; no business model or user directory"]
    fn export_reader_cli_fixtures() {
        let run =
            std::path::PathBuf::from(std::env::var_os("CERULEAN_READER_FIXTURE_DIR").unwrap());
        assert!(!run.exists(), "Preserve original fixture");
        fs::create_dir(&run).unwrap();
        let mut cases = Vec::new();
        for schema in [1, 2] {
            let case = run.join(format!("schema-{schema}"));
            fs::create_dir(&case).unwrap();
            let root = case.join("data");
            drop(Store::open(&root).unwrap());
            let archive1 = case.join("backup1");
            let (manifest1, hold1) = held(&root, &archive1);
            let (archive, manifest, hold) = if schema == 2 {
                let r = crate::data_format::review(&root, &archive1, &manifest1, &hold1).unwrap();
                crate::data_format::migrate(&root, &archive1, &manifest1, &hold1, &r.digest)
                    .unwrap();
                upgrade::release(&root, &hold1).unwrap();
                let archive = case.join("backup2");
                let (manifest, hold) = held(&root, &archive);
                (archive, manifest, hold)
            } else {
                (archive1, manifest1, hold1)
            };
            cases.push(serde_json::json!({"schema":schema,"data":root,"backup":archive,"manifestDigest":manifest,"holdDigest":hold}));
        }
        fs::write(
            run.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn promotion_preserves_original_bytes_and_requires_explicit_backup_review() {
        for schema in [1, 2] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            drop(Store::open(&root).unwrap());
            let mut archive = temp.path().join("backup1");
            let (mut manifest, mut hold) = held(&root, &archive);
            if schema == 2 {
                let r = crate::data_format::review(&root, &archive, &manifest, &hold).unwrap();
                crate::data_format::migrate(&root, &archive, &manifest, &hold, &r.digest).unwrap();
                upgrade::release(&root, &hold).unwrap();
                archive = temp.path().join("backup2");
                (manifest, hold) = held(&root, &archive);
            }
            let original = fs::read(root.join("upgrade-hold.json")).unwrap();
            let container = fs::read(root.join(crate::data_format::MARKER)).ok();
            assert!(require_override(&root, true).is_err());
            assert!(require_override(&root, false).is_ok());
            assert!(review(&root, &archive, &"0".repeat(64), &hold).is_err());
            let r = review(&root, &archive, &manifest, &hold).unwrap();
            assert!(promote(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
            assert!(!root.join(MARKER).exists());
            let result = promote(&root, &archive, &manifest, &hold, &r.digest).unwrap();
            assert_eq!(result.data_schema, schema);
            assert_eq!(result.minimum_reader_generation, CURRENT);
            assert_eq!(fs::read(root.join("upgrade-hold.json")).unwrap(), original);
            assert_eq!(
                fs::read(root.join(crate::data_format::MARKER)).ok(),
                container
            );
            assert!(promote(&root, &archive, &manifest, &hold, &r.digest).is_err());
            assert!(Store::open(&root).is_err());
            upgrade::release(&root, &hold).unwrap();
            require_override(&root, true).unwrap();
            drop(Store::open(&root).unwrap());
        }
    }
    #[test]
    fn invalid_future_partial_or_relocated_guard_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        promote_fixture(&root);
        let original = fs::read(root.join(MARKER)).unwrap();
        for field in ["schema", "minimumReaderGeneration", "dataSchema"] {
            let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
            record[field] = serde_json::json!(99);
            fs::write(root.join(MARKER), serde_json::to_vec(&record).unwrap()).unwrap();
            assert!(Store::open(&root).is_err());
        }
        let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
        record["unreviewed"] = true.into();
        fs::write(root.join(MARKER), serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(Store::open(&root).is_err());
        fs::write(root.join(MARKER), b"{").unwrap();
        assert!(Store::open(&root).is_err());
        fs::write(root.join(MARKER), &original).unwrap();
        let moved = temp.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        assert!(Store::open(&moved).is_err());
        fs::rename(&moved, &root).unwrap();
        let future = root.join("events/00000000.reader5.json");
        fs::write(&future, b"{}").unwrap();
        assert!(crate::data_format::status(&root).is_err());
        assert!(Store::open(&root).is_err());
        fs::remove_file(future).unwrap();
        fs::write(
            root.join("events/.format-migration-reader-incomplete.pending"),
            b"partial",
        )
        .unwrap();
        assert!(Store::open(&root).is_err());
    }
    #[test]
    fn unprotected_new_records_cannot_be_loaded_or_retroactively_promoted() {
        for (relative, record) in [
            (
                "project-setups/setup.json",
                serde_json::json!({"setup":{"profiles":[{"supervisionOverride":{"mode":"disabled"}}]}}),
            ),
            (
                "workflows/project/events/00000001.json",
                serde_json::json!({"body":{"context":{"execution":{"revision":{"execution":[{"supervisionOverride":{"mode":"disabled"}}]}}}}}),
            ),
            (
                "management-decisions/operation.2.json",
                serde_json::json!({"body":{"evidence":{"target":{"context":{"execution":{"revision":{"execution":[{"supervisionOverride":{"mode":"disabled"}}]}}}}}}}),
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            drop(Store::open(&root).unwrap());
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = serde_json::to_vec(&record).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(Store::open(&root).is_err());
            let archive = temp.path().join("backup");
            let (manifest, hold) = held(&root, &archive);
            assert!(review(&root, &archive, &manifest, &hold).is_err());
            assert!(!root.join(MARKER).exists());
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
    #[test]
    fn promotion_rejects_changed_source_active_writer_and_program_update() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let archive = temp.path().join("backup");
        let (manifest, hold) = held(&root, &archive);
        let lock = backup::open(&root.join("writer.lock"), false, true).unwrap();
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        drop(lock);
        let recovery = crate::backup_recovery::guard(&root).unwrap();
        let name = recovery.file_name().unwrap().to_string_lossy().replacen(
            ".cerulean-recovery-",
            ".cerulean-program-update-",
            1,
        );
        let interlock = recovery.with_file_name(format!("{name}.json"));
        fs::write(&interlock, b"{} ").unwrap();
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        fs::remove_file(interlock).unwrap();
        fs::write(root.join("changed"), b"preserve").unwrap();
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        assert!(!root.join(MARKER).exists());
    }
}

// Inspect only original round journals, not files returned by an API worker.
fn collect_api_rounds(workflows: &Path, paths: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !workflows.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    for workflow in fs::read_dir(workflows).map_err(|e| e.to_string())? {
        let workflow = workflow.map_err(|e| e.to_string())?.path();
        ordinary_dir(&workflow)?;
        for attempt in fs::read_dir(&workflow).map_err(|e| e.to_string())? {
            let attempt = attempt.map_err(|e| e.to_string())?;
            if !attempt
                .file_name()
                .to_string_lossy()
                .starts_with("api-attempt-")
            {
                continue;
            }
            ordinary_dir(&attempt.path())?;
            let rounds = attempt.path().join("rounds");
            if !rounds.try_exists().map_err(|e| e.to_string())? {
                continue;
            }
            ordinary_dir(&rounds)?;
            for round in fs::read_dir(&rounds).map_err(|e| e.to_string())? {
                let round = round.map_err(|e| e.to_string())?.path();
                ordinary_dir(&round)?;
                for entry in fs::read_dir(&round).map_err(|e| e.to_string())? {
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if path
                        .extension()
                        .is_some_and(|v| v == "json" || v == "pending")
                    {
                        paths.push(path);
                    }
                    if paths.len() > 100_000 {
                        return Err("Reader preflight history exceeds limit".into());
                    }
                }
            }
        }
    }
    Ok(())
}
