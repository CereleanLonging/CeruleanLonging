use super::*;
use cerulean_domain::credentials::ProviderEndpoint;
use reqwest::header::{HeaderName, HeaderValue};
use serde_json::json;

const SYNTHETIC_KEY: &str = "fixtureCredentialOnly012345";

fn target() -> ApiTarget {
    ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6Api {},
    }
}

fn capture_value(raw: &[u8], key: &str) -> GatewayHeaders {
    let mut headers = HeaderMap::new();
    headers.insert(HEADER_NAME, HeaderValue::from_bytes(raw).unwrap());
    capture(&target(), 200, &headers, key).unwrap()
}

fn accepted(value: &str) -> GatewayRequestId {
    GatewayRequestId::Accepted {
        value: value.into(),
    }
}

fn assert_rejected(raw: &[u8], reason: GatewayRequestIdRejection) {
    let observed = capture_value(raw, SYNTHETIC_KEY);
    assert_eq!(observed.request_id, GatewayRequestId::Rejected { reason });
    observed.validate().unwrap();
    let wire = serde_json::to_value(&observed).unwrap();
    assert!(wire["requestId"].get("value").is_none());
}

#[test]
fn gateway_missing_header_retains_status_and_ignores_unrelated_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-request-id",
        HeaderValue::from_static("unrelated-identity"),
    );
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer unrelated-secret"),
    );
    let observed = capture(&target(), 429, &headers, SYNTHETIC_KEY).unwrap();
    assert_eq!(observed.status, 429);
    assert_eq!(observed.request_id, GatewayRequestId::Missing {});
    observed.validate().unwrap();
    let wire = serde_json::to_string(&observed).unwrap();
    assert_eq!(wire, r#"{"status":429,"requestId":{"kind":"missing"}}"#);
    assert!(!format!("{observed:?}").contains("unrelated"));
}

#[test]
fn gateway_single_value_preserves_exact_allowed_ascii_at_both_bounds() {
    for value in ["x".to_owned(), "Ab012._:-Z".repeat(20)] {
        assert!((1..=200).contains(&value.len()));
        let observed = capture_value(value.as_bytes(), SYNTHETIC_KEY);
        assert_eq!(observed.request_id, accepted(&value));
        observed.validate().unwrap();
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_bytes(b"X-Oneapi-Request-Id").unwrap(),
        HeaderValue::from_static("Exact.Case:009-Id"),
    );
    let observed = capture(&target(), 503, &headers, SYNTHETIC_KEY).unwrap();
    assert_eq!(observed.status, 503);
    assert_eq!(observed.request_id, accepted("Exact.Case:009-Id"));
}

#[test]
fn gateway_non_a6_provider_is_not_applicable_even_with_candidate_headers() {
    let mut headers = HeaderMap::new();
    headers.append(HEADER_NAME, HeaderValue::from_static("id-one"));
    headers.append(HEADER_NAME, HeaderValue::from_static("id-two"));
    let targets = [
        ApiTarget::OpenAi {},
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Anthropic {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::KimiChina {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Grok {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenSingapore {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenHongKong {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenBeijing {
                workspace_id: "fixture".into(),
            },
        },
    ];
    for target in targets {
        assert!(capture(&target, 200, &headers, SYNTHETIC_KEY).is_none());
    }
}

#[test]
fn gateway_same_or_conflicting_repeated_headers_never_select_a_value() {
    for second in ["first-private-id", "different-private-id"] {
        let mut headers = HeaderMap::new();
        headers.append(HEADER_NAME, HeaderValue::from_static("first-private-id"));
        headers.append(HEADER_NAME, HeaderValue::from_str(second).unwrap());
        let observed = capture(&target(), 200, &headers, SYNTHETIC_KEY).unwrap();
        assert_eq!(
            observed.request_id,
            GatewayRequestId::Rejected {
                reason: GatewayRequestIdRejection::RepeatedHeader,
            }
        );
        observed.validate().unwrap();
        for output in [
            serde_json::to_string(&observed).unwrap(),
            format!("{observed:?}"),
        ] {
            assert!(!output.contains("first-private-id"));
            assert!(!output.contains(second));
        }
    }
}

#[test]
fn gateway_empty_or_oversized_header_is_rejected_without_truncation() {
    assert_rejected(b"", GatewayRequestIdRejection::InvalidLength);
    assert_rejected(&[b'a'; 201], GatewayRequestIdRejection::InvalidLength);
    let observed = capture_value(&[b'a'; 200], SYNTHETIC_KEY);
    assert_eq!(observed.request_id, accepted(&"a".repeat(200)));
}

#[test]
fn gateway_spaces_commas_unicode_and_other_punctuation_are_not_normalized() {
    for value in [
        " id",
        "id ",
        "id id",
        "id,other",
        "id\tother",
        "id/other",
        "id=other",
        "idé",
        "id%20other",
        "id\\other",
    ] {
        assert_rejected(
            value.as_bytes(),
            GatewayRequestIdRejection::InvalidCharacters,
        );
    }
}

#[test]
fn gateway_http_parser_and_auxiliary_validator_both_reject_line_breaks() {
    // HTTP HeaderMap cannot contain CR/LF. The independent disk-record reader
    // still has to reject them, rather than trusting the original HTTP parser.
    for value in ["id\r", "id\n", "id\r\nother", "id\0other", "id\u{7f}"] {
        assert!(HeaderValue::from_bytes(value.as_bytes()).is_err());
        let observed = GatewayHeaders {
            status: 200,
            request_id: accepted(value),
        };
        let error = observed.validate().unwrap_err();
        assert_eq!(error, "invalid gateway request identity");
        assert!(!error.contains(value));
        assert!(!format!("{observed:?}").contains(value));
    }
}

#[test]
fn gateway_exact_key_reflection_is_rejected_in_direct_and_embedded_positions() {
    for value in [
        SYNTHETIC_KEY.to_owned(),
        format!("before.{SYNTHETIC_KEY}"),
        format!("{SYNTHETIC_KEY}.after"),
        format!("before.{SYNTHETIC_KEY}.after"),
    ] {
        let observed = capture_value(value.as_bytes(), SYNTHETIC_KEY);
        assert_eq!(
            observed.request_id,
            GatewayRequestId::Rejected {
                reason: GatewayRequestIdRejection::CredentialReflection,
            }
        );
        assert!(!serde_json::to_string(&observed)
            .unwrap()
            .contains(SYNTHETIC_KEY));
        assert!(!format!("{observed:?}").contains(SYNTHETIC_KEY));
    }
    // Compare the complete key, never a prefix/digest; empty test input must
    // not trigger a zero-length windows panic or reject every ordinary ID.
    assert_eq!(
        capture_value(b"fixtureCredentialOnly", SYNTHETIC_KEY).request_id,
        accepted("fixtureCredentialOnly")
    );
    assert_eq!(
        capture_value(b"ordinary-id", "").request_id,
        accepted("ordinary-id")
    );
}

#[test]
fn gateway_credential_prefixes_are_rejected_in_capture_and_without_live_key() {
    for value in [
        "sk-not-a-real-key",
        "SK-TEST",
        "sK-Test",
        "Bearer",
        "Bearer-fixture",
        "bearerOpaque",
        "BEARER fixture",
    ] {
        assert_rejected(
            value.as_bytes(),
            GatewayRequestIdRejection::CredentialReflection,
        );
        let observed = GatewayHeaders {
            status: 200,
            request_id: accepted(value),
        };
        assert_eq!(
            observed.validate().unwrap_err(),
            "invalid gateway request identity"
        );
        assert!(!format!("{observed:?}").contains(value));
    }
}

#[test]
fn gateway_debug_never_exposes_an_accepted_id_even_for_unvalidated_objects() {
    for value in [
        "safe-but-private-request-identity",
        SYNTHETIC_KEY,
        "Bearer hidden",
    ] {
        let observed = GatewayHeaders {
            status: 200,
            request_id: accepted(value),
        };
        let debug = format!("{observed:?}");
        assert!(debug.contains("[SAFE_GATEWAY_ID]"));
        assert!(!debug.contains(value));
        assert!(!format!("{:?}", observed.request_id).contains(value));
    }
}

#[test]
fn gateway_serde_shape_is_closed_camel_case_and_round_trips() {
    let shapes = [
        json!({"status":200,"requestId":{"kind":"missing"}}),
        json!({"status":200,"requestId":{"kind":"accepted","value":"gateway-001"}}),
        json!({"status":400,"requestId":{"kind":"rejected","reason":"invalidLength"}}),
        json!({"status":400,"requestId":{"kind":"rejected","reason":"invalidCharacters"}}),
        json!({"status":400,"requestId":{"kind":"rejected","reason":"repeatedHeader"}}),
        json!({"status":400,"requestId":{"kind":"rejected","reason":"credentialReflection"}}),
    ];
    for shape in shapes {
        let observed: GatewayHeaders = serde_json::from_value(shape.clone()).unwrap();
        observed.validate().unwrap();
        assert_eq!(serde_json::to_value(&observed).unwrap(), shape);
        let mut extra_outer = shape.clone();
        extra_outer["headers"] = json!({"other":"forbidden"});
        assert!(serde_json::from_value::<GatewayHeaders>(extra_outer).is_err());
        let mut extra_inner = shape;
        extra_inner["requestId"]["raw"] = json!("forbidden");
        assert!(serde_json::from_value::<GatewayHeaders>(extra_inner).is_err());
    }
    for shape in [
        json!({"status":200,"request_id":{"kind":"missing"}}),
        json!({"status":200,"requestId":{"kind":"unknown"}}),
        json!({"status":200,"requestId":{"kind":"accepted"}}),
        json!({"status":200,"requestId":{"kind":"rejected","reason":"newReason"}}),
        json!({"status":200,"requestId":{"kind":"rejected","reason":{"credentialReflection":{"raw":"forbidden"}}}}),
    ] {
        assert!(serde_json::from_value::<GatewayHeaders>(shape).is_err());
    }
}

#[test]
fn gateway_auxiliary_validation_checks_status_and_untrusted_accepted_values() {
    for status in [0, 99, 600, u16::MAX] {
        let observed = GatewayHeaders {
            status,
            request_id: GatewayRequestId::Missing {},
        };
        assert_eq!(
            observed.validate().unwrap_err(),
            "invalid gateway response status"
        );
    }
    for status in [100, 200, 429, 599] {
        let observed = GatewayHeaders {
            status,
            request_id: accepted("valid-id"),
        };
        observed.validate().unwrap();
    }
    for value in [
        "".to_owned(),
        "a".repeat(201),
        " leading".into(),
        "trailing ".into(),
        "id,other".into(),
        "idé".into(),
    ] {
        let observed = GatewayHeaders {
            status: 200,
            request_id: accepted(&value),
        };
        assert_eq!(
            observed.validate().unwrap_err(),
            "invalid gateway request identity"
        );
    }
}
