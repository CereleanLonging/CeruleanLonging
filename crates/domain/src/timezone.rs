//! Pinned IANA rules, independent of the machine's timezone or Windows registry.
pub use chrono_tz::Tz;

pub const DEFAULT: &str = "Asia/Shanghai";
pub const TZDB_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;
const ARCHIVED_IANA_TZDB_VERSION: &str = "2025b";

/// Schema 2/3 handoff dates are derived from these exact IANA rules. Reject
/// an incompatible binary before it can rebuild or append an old projection.
pub fn require_archived_iana_rules() -> Result<(), String> {
    require_rules_version(TZDB_VERSION)
}

fn require_rules_version(actual: &str) -> Result<(), String> {
    if actual == ARCHIVED_IANA_TZDB_VERSION {
        Ok(())
    } else {
        Err(format!(
            "IANA timezone rules changed from {ARCHIVED_IANA_TZDB_VERSION} to {actual}; archived projects require an explicit migration"
        ))
    }
}

pub fn parse(name: &str) -> Result<Tz, String> {
    name.parse().map_err(|_| {
        "项目时区无效，请使用 IANA 名称，例如 Asia/Shanghai 或 America/New_York".into()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn timezone_schema_two_retains_pinned_database_rules() {
        // Changing rules can change archived day membership. Use a new explicit
        // configuration generation, not a silent dependency upgrade for schema 2.
        assert_eq!(super::TZDB_VERSION, super::ARCHIVED_IANA_TZDB_VERSION);
        assert!(super::require_archived_iana_rules().is_ok());
        assert!(super::require_rules_version("2026a")
            .unwrap_err()
            .contains("explicit migration"));
    }
}
