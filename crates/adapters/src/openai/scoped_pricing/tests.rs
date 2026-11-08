use super::*;
use cerulean_domain::currency_budget::{
    scoped::{BillingBasis, ContextMetric, PriceBand},
    Rates,
};

const AT: &str = "2026-10-16T00:00:00Z";
fn draft() -> ScopedDecisionDraft {
    ScopedDecisionDraft::standard(
        Purpose::Assign,
        "fixture-model",
        &"a".repeat(64),
        &json!({"scope":"private-context-marker"}),
    )
    .unwrap()
}
fn quote() -> ScopedPriceQuote {
    ScopedPriceQuote {
        count_price: None,
        schema: 1,
        target: ApiTarget::OpenAi {},
        requested_model: "fixture-model".into(),
        requested_service: RequestedService::Explicit {
            tier: "default".into(),
        },
        reported_models: vec!["gpt-test-snapshot".into()],
        reported_services: vec!["default".into()],
        billing_basis: BillingBasis::PerRequest,
        context_metric: ContextMetric::TotalInputIncludingCache,
        bands: vec![
            PriceBand {
                actual_model: "gpt-test-snapshot".into(),
                actual_service: "default".into(),
                input: InputRange {
                    from: 0,
                    through: 10,
                },
                rates: Rates {
                    input: 1000,
                    output: 2000,
                    cache_read: Some(100),
                    cache_write: Some(1500),
                },
            },
            PriceBand {
                actual_model: "gpt-test-snapshot".into(),
                actual_service: "default".into(),
                input: InputRange {
                    from: 11,
                    through: 100,
                },
                rates: Rates {
                    input: 2000,
                    output: 3000,
                    cache_read: Some(200),
                    cache_write: Some(3000),
                },
            },
        ],
        valid_from: "2026-10-15T00:00:00Z".into(),
        valid_until: "2026-10-17T00:00:00Z".into(),
        source: "synthetic rates for local tests only".into(),
    }
}
fn response() -> Value {
    let mut value = crate::openai::tests::response();
    value["model"] = json!("gpt-test-snapshot");
    value["service_tier"] = json!("default");
    value["usage"]["input_tokens_details"] = json!({"cached_tokens":3,"cache_write_tokens":4});
    value
}
fn review() -> ScopedPricingReview {
    draft()
        .review(
            &quote(),
            InputRange {
                from: 0,
                through: 100,
            },
            AT,
        )
        .unwrap()
}
fn amount(review: &ScopedPricingReview, value: &Value) -> u64 {
    match review.evaluate_response(value).unwrap().result {
        ScopedPricingResult::Priced { amount_usd_nanos } => amount_usd_nanos,
        ScopedPricingResult::Unpriced { reason } => panic!("unpriced: {reason}"),
    }
}
fn unpriced(review: &ScopedPricingReview, value: &Value) {
    assert!(matches!(
        review.evaluate_response(value).unwrap().result,
        ScopedPricingResult::Unpriced { .. }
    ));
}

#[test]
fn scoped_reasoning_preserves_exact_count_and_generation_configuration() {
    let context =
        json!({"executionSetup":{"setup":{"managementSources":{"deputyReasoningEffort":"high"}}}});
    let scoped = ScopedDecisionDraft::standard(
        Purpose::Assign,
        "gpt-6-astra",
        &digest(&context).unwrap(),
        &context,
    )
    .unwrap();
    assert_eq!(
        scoped.checked_count_body().unwrap()["reasoning"],
        json!({"effort":"high"})
    );
    for value in [
        json!("unreviewed"),
        json!({"effort":"high","summary":"auto"}),
    ] {
        let mut body = scoped.body.clone();
        body["reasoning"] = value;
        assert!(count_projection(&body).is_err());
    }
}

#[test]
fn scoped_draft_explicit_standard_has_distinct_generation_same_reviewed_count_context() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let context = json!({"scope":"private-context-marker"});
        let legacy =
            PreparedDecision::new(purpose, "fixture-model", &"a".repeat(64), &context).unwrap();
        let scoped =
            ScopedDecisionDraft::standard(purpose, "fixture-model", &"a".repeat(64), &context)
                .unwrap();
        assert_ne!(
            legacy.fingerprint().body_sha256,
            scoped.fingerprint().body_sha256
        );
        let (url, hash, bytes) = legacy.count_request_identity().unwrap();
        assert_eq!(
            (url, hash, bytes),
            (
                scoped.fingerprint.count_url.clone(),
                scoped.fingerprint.count_body_sha256.clone(),
                scoped.fingerprint.count_body_bytes
            )
        );
        assert_eq!(scoped.body["service_tier"], "default");
        let serialized = serde_json::to_string(scoped.fingerprint()).unwrap();
        assert!(!serialized.contains("private-context-marker"));
        // New standalone identity never decodes as the legacy send identity.
        assert!(serde_json::from_str::<ManagementRequestFingerprint>(&serialized).is_err());
    }
}

#[test]
fn scoped_draft_rejects_unreviewed_fields_and_changed_in_memory_body() {
    let mut d = draft();
    for (field, value) in [
        ("previous_response_id", json!("resp_old")),
        ("conversation", json!("conv_old")),
        ("prompt_cache_options", json!({"ttl":"24h"})),
    ] {
        let mut changed = d.body.clone();
        changed[field] = value;
        assert!(count_projection(&changed).is_err());
    }
    for (field, value) in [
        ("service_tier", json!("auto")),
        ("tools", json!([{"type":"web_search"}])),
        ("max_output_tokens", json!(8192)),
    ] {
        let mut changed = d.body.clone();
        changed[field] = value;
        assert!(count_projection(&changed).is_err());
    }
    d.body["input"] = json!("changed");
    assert!(d
        .review(
            &quote(),
            InputRange {
                from: 0,
                through: 100
            },
            AT
        )
        .is_err());
}

#[test]
fn scoped_review_requires_original_target_service_prices_and_valid_time() {
    let d = draft();
    let range = InputRange {
        from: 0,
        through: 100,
    };
    let mut q = quote();
    q.requested_service = RequestedService::Omitted;
    assert!(d.review(&q, range.clone(), AT).is_err());
    q = quote();
    q.bands[0].rates.cache_write = None;
    assert!(d.review(&q, range.clone(), AT).is_err());
    q = quote();
    q.bands[1].input.from = 12;
    assert!(d.review(&q, range.clone(), AT).is_err());
    assert!(d
        .review(&quote(), range.clone(), "2026-10-17T00:00:00Z")
        .is_err());
    assert!(d.review(&quote(), range, "2026-10-14T23:59:59Z").is_err());
}

#[test]
fn scoped_review_prices_full_request_and_separate_cache_counters_at_boundary() {
    let review = review();
    assert_eq!(review.audit().maximum_usd_nanos, 12_588);
    let mut value = response();
    assert_eq!(amount(&review, &value), 50);
    value["usage"]["input_tokens"] = json!(11);
    value["usage"]["input_tokens_details"]["cached_tokens"] = json!(4);
    assert_eq!(amount(&review, &value), 79);
    // The whole output uses the long-context rate, not only input above T.
    value["usage"]["input_tokens"] = json!(101);
    unpriced(&review, &value);
    value = response();
    value["usage"]["output_tokens"] = json!(4097);
    unpriced(&review, &value);
}

#[test]
fn scoped_response_never_fills_missing_actual_tier_or_cache_with_defaults() {
    let review = review();
    for tier in [
        Value::Null,
        json!("auto"),
        json!("priority"),
        json!("fast"),
        json!("unknown-new-tier"),
        json!(7),
    ] {
        let mut value = response();
        value["service_tier"] = tier;
        unpriced(&review, &value);
    }
    for field in ["cached_tokens", "cache_write_tokens"] {
        for invalid in [Value::Null, json!("0"), json!(-1), json!(1.5), json!(11)] {
            let mut value = response();
            value["usage"]["input_tokens_details"][field] = invalid;
            unpriced(&review, &value);
        }
    }
    let mut value = response();
    value["usage"]["input_tokens_details"] = json!({"cached_tokens":0,"cache_write_tokens":0});
    assert_eq!(amount(&review, &value), 50);
    value["model"] = json!("unapproved-model");
    unpriced(&review, &value);
}

#[test]
fn scoped_billing_is_independent_of_decision_success_and_exposes_no_private_text() {
    let review = review();
    let mut value = response();
    value["output"] = json!([{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"invalid-decision-private-marker"}]}]);
    let result = review.evaluate_response(&value).unwrap();
    assert!(!result.decision_usable);
    assert_eq!(
        result.result,
        ScopedPricingResult::Priced {
            amount_usd_nanos: 50
        }
    );
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("private-marker"));
    assert_eq!(result.response_sha256, digest(&value).unwrap());
    value["status"] = json!("incomplete");
    assert_eq!(amount(&review, &value), 50);
    value["id"] = Value::Null;
    unpriced(&review, &value);
}

#[test]
fn scoped_review_owns_original_quote_and_binds_interval_and_response_digest() {
    let d = draft();
    let mut q = quote();
    let original = d
        .review(
            &q,
            InputRange {
                from: 0,
                through: 100,
            },
            AT,
        )
        .unwrap();
    q.bands[0].rates.output = 7000;
    let changed = d
        .review(
            &q,
            InputRange {
                from: 0,
                through: 100,
            },
            AT,
        )
        .unwrap();
    assert_ne!(original.audit().quote_sha256, changed.audit().quote_sha256);
    assert_eq!(amount(&original, &response()), 50);
    assert_eq!(amount(&changed, &response()), 150);
    let narrower = d
        .review(
            &quote(),
            InputRange {
                from: 10,
                through: 10,
            },
            AT,
        )
        .unwrap();
    assert_ne!(
        original.audit().pricing_request_sha256,
        narrower.audit().pricing_request_sha256
    );
    let mut value = response();
    value["usage"]["input_tokens"] = json!(9);
    unpriced(&narrower, &value);
    assert_ne!(
        original
            .evaluate_response(&response())
            .unwrap()
            .response_sha256,
        original.evaluate_response(&value).unwrap().response_sha256
    );
}
