use super::*;
use crate::{credentials::ProviderEndpoint, currency_budget::PriceQuote};

const NOW: &str = "2026-10-16T12:00:00+08:00";

fn rates(input: u64) -> Rates {
    Rates {
        input,
        output: 2_000,
        cache_read: Some(500),
        cache_write: Some(3_000),
    }
}
fn band(from: u64, through: u64, input: u64) -> PriceBand {
    PriceBand {
        actual_model: "fixture-snapshot".into(),
        actual_service: "default".into(),
        input: InputRange { from, through },
        rates: rates(input),
    }
}
fn quote() -> ScopedPriceQuote {
    ScopedPriceQuote {
        count_price: None,
        schema: 1,
        target: ApiTarget::OpenAi {},
        requested_model: "fixture-alias".into(),
        requested_service: RequestedService::Explicit {
            tier: "default".into(),
        },
        reported_models: vec!["fixture-snapshot".into()],
        reported_services: vec!["default".into()],
        billing_basis: BillingBasis::PerRequest,
        context_metric: ContextMetric::TotalInputIncludingCache,
        bands: vec![band(0, 1_000, 1_000)],
        valid_from: "2026-10-16T00:00:00+08:00".into(),
        valid_until: "2026-10-17T00:00:00+08:00".into(),
        source: "Synthetic reviewed terms; no supplier price or billing guarantee".into(),
    }
}
fn request(from: u64, through: u64) -> PricingRequest {
    PricingRequest {
        target: ApiTarget::OpenAi {},
        requested_model: "fixture-alias".into(),
        requested_service: RequestedService::Explicit {
            tier: "default".into(),
        },
        input: InputRange { from, through },
        output_ceiling: 10,
        cache_read_allowed: false,
        cache_write_allowed: false,
    }
}
fn observation(input: u64, output: u64) -> PricingObservation {
    PricingObservation {
        actual_model: "fixture-snapshot".into(),
        actual_service: Some("default".into()),
        usage: Usage {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
        },
    }
}

#[test]
fn interval_boundaries_select_exactly_one_price() {
    let mut quote = quote();
    quote.bands = vec![band(0, 100, 1_000), band(101, 1_000, 2_000)];
    for (input, expected) in [(99, 119), (100, 120), (101, 222)] {
        let request = request(input, input);
        assert_eq!(quote.reserve(&request, NOW).unwrap(), expected);
        assert_eq!(
            quote.charge(&request, &observation(input, 10)).unwrap(),
            expected
        );
    }
}

#[test]
fn nonmonotonic_bands_use_their_own_reachable_upper_bound() {
    let mut quote = quote();
    quote.bands = vec![band(0, 100, 10_000), band(101, 200, 1_000)];
    let wide = request(10, 200);
    // Short expensive band: 100 * 10 + output 20, not 200 * 10 + 20.
    assert_eq!(quote.reserve(&wide, NOW).unwrap(), 1_020);
    assert_eq!(quote.charge(&wide, &observation(100, 10)).unwrap(), 1_020);
    assert_eq!(quote.charge(&wide, &observation(200, 10)).unwrap(), 220);
    assert_eq!(quote.reserve(&request(101, 200), NOW).unwrap(), 220);
    quote.bands.reverse();
    assert_eq!(quote.reserve(&wide, NOW).unwrap(), 1_020);
}

#[test]
fn complete_cross_product_of_models_and_services_is_covered() {
    let mut quote = quote();
    quote.reported_models.push("fixture-other".into());
    quote.reported_services.push("priority".into());
    quote.bands.clear();
    for (model, service, input_rate) in [
        ("fixture-snapshot", "default", 1_000),
        ("fixture-snapshot", "priority", 5_000),
        ("fixture-other", "default", 2_000),
        ("fixture-other", "priority", 9_000),
    ] {
        let mut value = band(0, 1_000, input_rate);
        value.actual_model = model.into();
        value.actual_service = service.into();
        quote.bands.push(value);
    }
    let request = request(10, 20);
    assert_eq!(quote.reserve(&request, NOW).unwrap(), 200);
    let mut observed = observation(10, 1);
    assert_eq!(quote.charge(&request, &observed).unwrap(), 12);
    observed.actual_model = "fixture-other".into();
    observed.actual_service = Some("priority".into());
    assert_eq!(quote.charge(&request, &observed).unwrap(), 92);
    quote.bands.pop();
    assert!(quote.validate().is_err());
    assert!(quote.reserve(&request, NOW).is_err());
}

#[test]
fn gaps_overlaps_and_undeclared_bands_cannot_price_a_request() {
    let mut quote = quote();
    quote.bands = vec![band(0, 49, 1_000), band(51, 1_000, 2_000)];
    quote.validate().unwrap();
    assert!(quote.reserve(&request(10, 60), NOW).is_err());
    assert!(quote.charge(&request(10, 60), &observation(20, 1)).is_err());
    assert_eq!(quote.reserve(&request(0, 49), NOW).unwrap(), 69);
    quote.bands[1].input.from = 49;
    assert!(quote.validate().is_err());
    quote.bands[1].input.from = 50;
    quote.bands[1].actual_service = "undeclared".into();
    assert!(quote.validate().is_err());
}

#[test]
fn cached_input_participates_in_context_selection_and_exact_charge() {
    let mut quote = quote();
    quote.bands = vec![band(0, 100, 1_000), band(101, 200, 2_000)];
    quote.bands[1].rates.cache_read = Some(200);
    quote.bands[1].rates.cache_write = Some(4_000);
    let mut request = request(100, 200);
    request.cache_read_allowed = true;
    request.cache_write_allowed = true;
    assert_eq!(quote.reserve(&request, NOW).unwrap(), 820);
    let mut observed = observation(90, 10);
    observed.usage.cache_read = 11;
    // Full input is 101, although ordinary uncached input is only 90.
    // ceil((90 * 2000 + 11 * 200 + 10 * 2000) / 1000) = 203.
    assert_eq!(quote.charge(&request, &observed).unwrap(), 203);
    observed.usage.cache_read = 0;
    observed.usage.cache_write = 11;
    assert_eq!(quote.charge(&request, &observed).unwrap(), 244);
}

#[test]
fn unknown_cache_rates_are_not_free_or_ignored_on_another_band() {
    let mut quote = quote();
    quote.bands = vec![band(0, 100, 1_000), band(101, 200, 2_000)];
    quote.bands[1].rates.cache_read = None;
    let mut request = request(0, 200);
    request.cache_read_allowed = true;
    assert!(quote.reserve(&request, NOW).is_err());
    assert!(quote.charge(&request, &observation(10, 1)).is_err());
    request.input.through = 100;
    assert!(quote.reserve(&request, NOW).is_ok());
    quote.bands[0].rates.cache_write = None;
    request.cache_write_allowed = true;
    assert!(quote.reserve(&request, NOW).is_err());
    request.cache_write_allowed = false;
    assert!(quote.reserve(&request, NOW).is_ok());
    let mut observed = observation(10, 1);
    observed.usage.cache_write = 1;
    assert!(quote.charge(&request, &observed).is_err());
}

#[test]
fn omitted_requested_and_actual_service_are_never_defaults() {
    let mut quote = quote();
    let explicit = request(0, 100);
    let mut omitted = explicit.clone();
    omitted.requested_service = RequestedService::Omitted;
    assert!(quote.reserve(&omitted, NOW).is_err());
    quote.requested_service = RequestedService::Omitted;
    assert!(quote.reserve(&explicit, NOW).is_err());
    assert!(quote.reserve(&omitted, NOW).is_ok());
    let mut observed = observation(10, 1);
    assert!(quote.charge(&omitted, &observed).is_ok());
    for actual in [
        None,
        Some("auto"),
        Some("fast"),
        Some("priority"),
        Some("unknown"),
    ] {
        observed.actual_service = actual.map(str::to_owned);
        assert!(quote.charge(&omitted, &observed).is_err(), "{actual:?}");
    }
    quote.requested_service = RequestedService::Explicit {
        tier: "auto".into(),
    };
    omitted.requested_service = quote.requested_service.clone();
    observed.actual_service = Some("default".into());
    assert!(quote.charge(&omitted, &observed).is_ok());
    quote.reported_services = vec!["auto".into()];
    quote.bands[0].actual_service = "auto".into();
    assert!(quote.validate().is_err());
}

#[test]
fn fast_and_priority_remain_distinct_exact_reported_services() {
    let mut quote = quote();
    quote.reported_services = vec!["fast".into()];
    quote.bands[0].actual_service = "fast".into();
    let request = request(0, 100);
    let mut observed = observation(10, 1);
    observed.actual_service = Some("fast".into());
    assert_eq!(quote.charge(&request, &observed).unwrap(), 12);
    observed.actual_service = Some("priority".into());
    assert!(quote.charge(&request, &observed).is_err());
    quote.reported_services = vec!["priority".into()];
    quote.bands[0].actual_service = "priority".into();
    assert_eq!(quote.charge(&request, &observed).unwrap(), 12);
    observed.actual_service = Some("fast".into());
    assert!(quote.charge(&request, &observed).is_err());
}

#[test]
fn original_endpoint_model_and_case_sensitive_actual_names_are_bound() {
    let quote = quote();
    let mut request = request(0, 100);
    request.requested_model = "other-model".into();
    assert!(quote.reserve(&request, NOW).is_err());
    request.requested_model = quote.requested_model.clone();
    let mut observed = observation(10, 1);
    observed.actual_model = quote.requested_model.clone();
    assert!(quote.charge(&request, &observed).is_err());
    observed.actual_model = "Fixture-snapshot".into();
    assert!(quote.charge(&request, &observed).is_err());
    observed.actual_model = "fixture-snapshot".into();
    observed.actual_service = Some("Default".into());
    assert!(quote.charge(&request, &observed).is_err());
    for endpoint in [
        ProviderEndpoint::Anthropic {},
        ProviderEndpoint::DeepSeek {},
    ] {
        request.target = ApiTarget::Provider { endpoint };
        assert!(request.validate().is_err());
        let mut other = quote.clone();
        other.target = request.target.clone();
        assert!(other.validate().is_err());
        assert!(other.reserve(&request, NOW).is_err());
    }
}

#[test]
fn lower_upper_output_and_cache_permissions_all_bound_observations() {
    let quote = quote();
    let request = request(10, 100);
    for invalid in [observation(9, 1), observation(101, 1), observation(10, 11)] {
        assert!(quote.charge(&request, &invalid).is_err());
    }
    for (read, write) in [(1, 0), (0, 1)] {
        let mut observed = observation(10, 1);
        observed.usage.cache_read = read;
        observed.usage.cache_write = write;
        assert!(quote.charge(&request, &observed).is_err());
    }
    assert_eq!(quote.charge(&request, &observation(10, 0)).unwrap(), 10);
}

#[test]
fn validity_uses_instants_and_does_not_reprice_late_responses() {
    let quote = quote();
    let request = request(0, 100);
    assert!(quote.reserve(&request, "2026-10-15T15:59:59Z").is_err());
    assert!(quote.reserve(&request, "2026-10-15T16:00:00Z").is_ok());
    assert!(quote.reserve(&request, "2026-10-16T15:59:59Z").is_ok());
    assert!(quote.reserve(&request, "2026-10-16T16:00:00Z").is_err());
    assert!(quote.reserve(&request, "not-a-time").is_err());
    // Pure settlement uses original terms. The host must prove its original
    // valid reservation; wall-clock time never substitutes a newer quote.
    assert_eq!(quote.charge(&request, &observation(10, 1)).unwrap(), 12);
}

#[test]
fn integer_rounding_is_once_per_request_and_limits_do_not_overflow() {
    let mut quote = quote();
    quote.bands[0].rates = Rates {
        input: 1,
        output: 1,
        cache_read: Some(1),
        cache_write: Some(1),
    };
    let mut request = request(0, 3);
    request.output_ceiling = 1;
    request.cache_read_allowed = true;
    request.cache_write_allowed = true;
    let mut observed = observation(1, 1);
    observed.usage.cache_read = 1;
    observed.usage.cache_write = 1;
    assert_eq!(quote.reserve(&request, NOW).unwrap(), 1);
    assert_eq!(quote.charge(&request, &observed).unwrap(), 1);
    quote.bands[0].input.through = MAX_TOKENS;
    quote.bands[0].rates.input = 1_000_000_000_000;
    request.input.through = MAX_TOKENS;
    assert!(quote.reserve(&request, NOW).is_err());
    request.input.through = u64::MAX;
    assert!(request.validate().is_err());
    request.input.through = 3;
    request.output_ceiling = 0;
    assert!(request.validate().is_err());
    quote.bands[0].rates.input = u64::MAX;
    assert!(quote.validate().is_err());
}

#[test]
fn malformed_or_ambiguous_contracts_are_rejected() {
    let original = quote();
    for mutation in 0..8 {
        let mut value = original.clone();
        match mutation {
            0 => value.schema = 2,
            1 => value.source.clear(),
            2 => value.source = "two\nlines".into(),
            3 => value.valid_until = value.valid_from.clone(),
            4 => value.reported_models.push(value.reported_models[0].clone()),
            5 => value
                .reported_services
                .push(value.reported_services[0].clone()),
            6 => {
                value.bands[0].input = InputRange {
                    from: 10,
                    through: 9,
                }
            }
            7 => value.bands.push(value.bands[0].clone()),
            _ => unreachable!(),
        }
        assert!(value.validate().is_err(), "mutation {mutation}");
    }
    let mut value = serde_json::to_value(original).unwrap();
    value["billingBasis"] = "session".into();
    assert!(serde_json::from_value::<ScopedPriceQuote>(value.clone()).is_err());
    value["billingBasis"] = "perRequest".into();
    value["contextMetric"] = "uncachedInput".into();
    assert!(serde_json::from_value::<ScopedPriceQuote>(value.clone()).is_err());
    value["contextMetric"] = "totalInputIncludingCache".into();
    value["assumeMissingTierIsDefault"] = true.into();
    assert!(serde_json::from_value::<ScopedPriceQuote>(value).is_err());
}

#[test]
fn independent_contract_does_not_change_legacy_quote_bytes() {
    let source = r#"{"schema":1,"target":{"kind":"openAi"},"requestedModel":"fixture-alias","reportedModels":["fixture-snapshot"],"rates":{"input":1000,"output":2000,"cacheRead":null,"cacheWrite":null},"validFrom":"2026-10-16T00:00:00+08:00","validUntil":"2026-10-17T00:00:00+08:00","source":"legacy fixture"}"#;
    let legacy: PriceQuote = serde_json::from_str(source).unwrap();
    legacy.validate().unwrap();
    assert_eq!(serde_json::to_string(&legacy).unwrap(), source);
    assert!(serde_json::from_str::<ScopedPriceQuote>(source).is_err());
    let scoped = serde_json::to_string(&quote()).unwrap();
    assert!(serde_json::from_str::<PriceQuote>(&scoped).is_err());
}

fn settings() -> super::super::BudgetSettings {
    super::super::BudgetSettings {
        schema: 2,
        limit_usd_nanos: 1_000_000,
        quotes: vec![],
        scoped_quotes: Some(vec![quote()]),
    }
}

#[test]
fn scoped_budget_schema_and_exact_policy_binding_are_not_interchangeable() {
    use crate::execution::digest;
    let settings = settings();
    settings.validate().unwrap();
    let policy = settings.bind("project", &"a".repeat(64)).unwrap();
    let hash = digest(&quote()).unwrap();
    assert_eq!(policy.scoped_quote(&hash).unwrap(), &quote());
    assert!(policy.quote(&hash).is_err());
    assert_eq!(policy.scoped_quotes, settings.scoped_quotes);
    let planning = settings
        .bind_planning("project", &"b".repeat(64), "fixture-alias")
        .unwrap();
    assert_eq!(planning.scoped_quotes, settings.scoped_quotes);
    assert!(settings
        .bind_planning("project", &"b".repeat(64), "unpriced-chief")
        .is_err());
    assert!(settings
        .require_model(&ApiTarget::OpenAi {}, "fixture-alias")
        .is_ok());
    assert!(settings
        .require_model(&ApiTarget::OpenAi {}, "other")
        .is_err());
    for mutation in 0..5 {
        let mut invalid = settings.clone();
        match mutation {
            0 => invalid.schema = 1,
            1 => invalid.scoped_quotes = None,
            2 => invalid.scoped_quotes = Some(vec![]),
            3 => invalid.scoped_quotes.as_mut().unwrap().push(quote()),
            4 => invalid.schema = 3,
            _ => unreachable!(),
        }
        assert!(invalid.validate().is_err(), "mutation {mutation}");
    }
    let legacy = PriceQuote {
        count_price: None,
        schema: 1,
        target: ApiTarget::OpenAi {},
        requested_model: "fixture-alias".into(),
        reported_models: vec!["fixture-snapshot".into()],
        rates: rates(1_000),
        valid_from: quote().valid_from,
        valid_until: quote().valid_until,
        source: "synthetic legacy".into(),
    };
    let mut mixed = settings;
    mixed.quotes.push(legacy);
    assert!(mixed.validate().is_err());
    mixed.schema = 1;
    assert!(mixed.validate().is_err());
    mixed.scoped_quotes = None;
    let legacy_policy = mixed.bind("project", &"a".repeat(64)).unwrap();
    assert!(legacy_policy.scoped_quote(&hash).is_err());
    assert!(legacy_policy
        .quote(&digest(&mixed.quotes[0]).unwrap())
        .is_ok());
}

fn reservation() -> super::super::Reservation {
    super::super::Reservation {
        pricing: Some(request(0, 100)),
        request_id: "request".into(),
        operation_id: "operation".into(),
        role: super::super::ChargeRole::Worker,
        quote_digest: "a".repeat(64),
        request_sha256: "b".repeat(64),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 10,
            cache_read_allowed: false,
            cache_write_allowed: false,
        },
        ceiling_evidence_sha256: "c".repeat(64),
    }
}

#[test]
fn scoped_reservation_must_match_all_original_ceiling_dimensions() {
    let original = reservation();
    original.validate().unwrap();
    for mutation in 0..5 {
        let mut invalid = original.clone();
        let pricing = invalid.pricing.as_mut().unwrap();
        match mutation {
            0 => pricing.input.through -= 1,
            1 => pricing.output_ceiling -= 1,
            2 => pricing.cache_read_allowed = true,
            3 => pricing.cache_write_allowed = true,
            4 => pricing.input.from = 101,
            _ => unreachable!(),
        }
        assert!(invalid.validate().is_err(), "mutation {mutation}");
    }
}

#[test]
fn settlement_retains_unknown_or_unpriced_service_without_defaulting() {
    let mut settlement = super::super::Settlement {
        actual_service: None,
        request_id: "request".into(),
        response_id: "response".into(),
        request_sha256: "b".repeat(64),
        actual_model: "fixture-snapshot".into(),
        usage: observation(10, 1).usage,
        evidence_sha256: "c".repeat(64),
    };
    settlement.validate().unwrap();
    assert!(serde_json::to_value(&settlement)
        .unwrap()
        .get("actualService")
        .is_none());
    for actual in ["default", "future_unpriced.tier-1", "auto"] {
        settlement.actual_service = Some(actual.into());
        settlement.validate().unwrap();
        let restored: super::super::Settlement =
            serde_json::from_value(serde_json::to_value(&settlement).unwrap()).unwrap();
        assert_eq!(restored.actual_service.as_deref(), Some(actual));
    }
    for actual in [
        String::new(),
        "tier\nvalue".into(),
        "tier value".into(),
        "a".repeat(65),
    ] {
        settlement.actual_service = Some(actual);
        assert!(settlement.validate().is_err());
    }
}

#[test]
fn scoped_count_price_is_optional_explicit_and_validated() {
    let mut value = quote();
    let original = serde_json::to_string(&value).unwrap();
    assert!(!original.contains("countPrice"));
    value.count_price = Some(ManagementCountPrice {
        schema: 1,
        maximum_usd_nanos: 0,
        maximum_body_bytes: 2048,
        source: "explicit synthetic reviewed zero; no supplier guarantee".into(),
    });
    value.validate().unwrap();
    assert_eq!(value.count_price.as_ref().unwrap().maximum_usd_nanos, 0);
    value.count_price.as_mut().unwrap().source.clear();
    assert!(value.validate().is_err());
    value.count_price = None;
    assert_eq!(serde_json::to_string(&value).unwrap(), original);
}

#[test]
fn legacy_settings_policy_reservation_and_settlement_bytes_stay_exact() {
    use crate::currency_budget::{BudgetPolicy, BudgetSettings, Reservation, Settlement};
    let quote = r#"{"schema":1,"target":{"kind":"openAi"},"requestedModel":"fixture-alias","reportedModels":["fixture-snapshot"],"rates":{"input":1000,"output":2000,"cacheRead":null,"cacheWrite":null},"validFrom":"2026-10-16T00:00:00+08:00","validUntil":"2026-10-17T00:00:00+08:00","source":"legacy fixture"}"#;
    let settings = format!(r#"{{"schema":1,"limitUsdNanos":1000000,"quotes":[{quote}]}}"#);
    let restored: BudgetSettings = serde_json::from_str(&settings).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), settings);
    let policy = format!(
        r#"{{"schema":1,"projectId":"project","revisionDigest":"{}","limitUsdNanos":1000000,"quotes":[{quote}]}}"#,
        "a".repeat(64)
    );
    let restored: BudgetPolicy = serde_json::from_str(&policy).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), policy);
    let reservation = format!(
        r#"{{"requestId":"request","operationId":"operation","role":"worker","quoteDigest":"{}","requestSha256":"{}","ceiling":{{"totalInput":100,"output":10,"cacheReadAllowed":false,"cacheWriteAllowed":false}},"ceilingEvidenceSha256":"{}"}}"#,
        "a".repeat(64),
        "b".repeat(64),
        "c".repeat(64)
    );
    let restored: Reservation = serde_json::from_str(&reservation).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), reservation);
    let settlement = format!(
        r#"{{"requestId":"request","responseId":"response","requestSha256":"{}","actualModel":"fixture-snapshot","usage":{{"input":10,"output":1,"cacheRead":0,"cacheWrite":0}},"evidenceSha256":"{}"}}"#,
        "b".repeat(64),
        "c".repeat(64)
    );
    let restored: Settlement = serde_json::from_str(&settlement).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_string(&restored).unwrap(), settlement);
}
