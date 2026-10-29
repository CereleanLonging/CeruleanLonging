"""Normalize recorded usage, retain unknowns, reject duplicate response identities.

Reads a hash-selected 242/243 export; does not attest its original source,
authorize work, produce an accepted experiment record, or calculate money.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

MAX = (1 << 64) - 1
FIELDS = ("inputTokens", "outputTokens", "cachedInputTokens", "cacheWriteTokens")


def require(value, message):
    if not value:
        raise ValueError(message)


def number(value, name, optional=True):
    if value is None and optional:
        return None
    require(type(value) is int and 0 <= value <= MAX, "Invalid counter: " + name)
    return value


def add(values):
    if any(v is None for v in values):
        return None
    total = sum(values)
    require(total <= MAX, "Counter overflow")
    return total


def target_provider(target):
    if target == {"kind": "openAi"}:
        return "openai"
    require(target.get("kind") == "provider", "Unsupported API target")
    endpoint = target["endpoint"]
    providers = {"anthropic": "anthropic", "deepSeek": "deepseek", "kimiChina": "kimi",
                 "grok": "grok", "qwenBeijing": "qwen", "qwenHongKong": "qwen",
                 "qwenSingapore": "qwen"}
    require(endpoint.get("kind") in providers, "Unsupported provider endpoint")
    return providers[endpoint["kind"]]


def normalized(provider, basis, usage):
    empty = {field: None for field in FIELDS}
    if usage is None:
        return empty
    require(isinstance(usage, dict), "Usage must be object or null")
    if basis == "scoped-normalized-usage":
        require(provider == "openai", "Unexpected scoped usage provider")
        require(set(usage) == {"input", "output", "cacheRead", "cacheWrite"}, "Scoped usage shape")
        values = {k: number(v, k, False) for k, v in usage.items()}
        return dict(inputTokens=add([values["input"], values["cacheRead"], values["cacheWrite"]]),
                    outputTokens=values["output"], cachedInputTokens=values["cacheRead"],
                    cacheWriteTokens=values["cacheWrite"])
    require(basis in ("provider-api-observation", "legacy-observation-without-wire-fingerprint"),
            "Unknown usage basis")
    require(set(usage) == {"inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens"},
            "Provider usage shape")
    input_count = number(usage["inputTokens"], "input", False)
    output = number(usage["outputTokens"], "output", False)
    read = number(usage["cacheReadTokens"], "cache read")
    write = number(usage["cacheWriteTokens"], "cache write")
    if provider == "anthropic":
        # Anthropic's input_tokens excludes both cache classes. Missing cache
        # details mean inclusive input is unknown, not input_tokens + zero.
        inclusive = add([input_count, read, write])
    else:
        inclusive = input_count
        require(read is None or read <= input_count, "Cached input exceeds total")
        if provider == "openai":
            require(write is None or write <= input_count, "Cache writes exceed total")
            require(read is None or write is None or read + write <= input_count,
                    "Combined cache counts exceed input")
        else:
            require(write is None, "Unsupported provider cache-write counter")
    return dict(inputTokens=inclusive, outputTokens=output,
                cachedInputTokens=read, cacheWriteTokens=write)


def totals(rows, source_missing=False):
    result = {}
    for field in FIELDS:
        values = [r["normalizedUsage"][field] for r in rows]
        known = add([v for v in values if v is not None])
        missing = sum(v is None for v in values)
        result[field] = dict(knownSubtotal=known, missingScopes=missing,
                             total=known if not missing and not source_missing else None)
    return result


def analyze(export):
    require(export["schema"] in (1, 2) and export["kind"] == "original-worker-and-verification-evidence",
            "Unsupported export")
    require(export["ledgerAndApiChainsVerified"] is True, "Upstream ledger check missing")
    project = export["projectId"]
    rows = []
    ids = set()
    responses = set()
    differences = []

    def append(channel, identity, provider, basis, usage, requested, actual, response, state):
        require((channel, identity) not in ids, "Duplicate operation/request identity")
        ids.add((channel, identity))
        if response is not None:
            require(isinstance(response, str) and re.fullmatch("[a-f0-9]{64}", response),
                    "Invalid response digest")
            require((provider, response) not in responses, "Duplicate response across recorded scopes")
            responses.add((provider, response))
        if usage is not None:
            require(response is not None and isinstance(actual, str) and actual,
                    "Observed usage lacks response/model identity")
        if actual is not None and actual != requested:
            differences.append(dict(channel=channel, id=identity, requested=requested, actual=actual))
        rows.append(dict(channel=channel, id=identity, provider=provider, usageBasis=basis,
                         state=state, sourceUsage=usage,
                         normalizedUsage=normalized(provider, basis, usage)))

    outcomes = {"reply", "not-sent", "failure-remote-usage-unknown", "intent-without-outcome", "rejected-with-observed-usage"}
    for call in export["apiCalls"]:
        identity = call["identity"]
        require(identity["projectId"] == project, "Worker evidence belongs to another project")
        require(call["outcome"] in outcomes, "Unknown worker outcome")
        require(call["outcome"] != "rejected-with-observed-usage" or export["schema"] == 2, "Rejected measurement needs export schema 2")
        require((call["outcome"] in ("reply", "rejected-with-observed-usage")) == (call["usage"] is not None), "Outcome/usage mismatch")
        fingerprint = identity["fingerprint"]
        append("worker", identity["requestId"], target_provider(fingerprint["target"]),
               "provider-api-observation", call["usage"], fingerprint["requestedModel"],
               call["actualModel"], call["responseIdDigest"], call["outcome"])
    require(type(export["apiIntentCount"]) is int
            and export["apiIntentCount"] == len(export["apiCalls"]), "API inventory count mismatch")
    included = export["managementUsageIncluded"]
    require(type(included) is bool, "Missing management inclusion classification")
    reviews = []
    if included:
        management = export["management"]
        require(management["projectId"] == project and management["managementChainsVerified"] is True
                and management["workflowApplicationsCrossChecked"] is True, "Management evidence not checked")
        for op in management["operations"]:
            provider = op["source"]
            require(provider in ("openai", "deepseek", "codex-session"), "Unsupported management source")
            if provider == "codex-session":
                require(op["usage"] is None and op["productApiIntent"] is False
                        and op["usageBasis"] == "external-session-usage-unknown",
                        "External session usage cannot be invented")
            else:
                require(op["productApiIntent"] is True, "API source identity mismatch")
            append("management", op["operationId"], provider, op["usageBasis"], op["usage"],
                   op["requestedModel"], op["actualModel"], op["responseIdDigest"], op["state"])
        reviews = management["reviewedFragments"]
    missing_source = not included or bool(reviews)
    groups = []
    for provider in sorted({r["provider"] for r in rows}):
        selected = [r for r in rows if r["provider"] == provider]
        groups.append(dict(provider=provider, recordedScopes=len(selected),
                           coverage="exported-scopes-only",
                           counters=totals(selected, missing_source)))
    counters = totals(rows, missing_source)
    return dict(schema=1, kind="recorded-usage-normalization", projectId=project,
        ledgerHead=export["ledgerHead"], revisionDigest=export["revisionDigest"],
        recordedScopes=len(rows), workerScopes=sum(r["channel"] == "worker" for r in rows),
        managementScopes=sum(r["channel"] == "management" for r in rows),
        managementIncluded=included, unresolvedReviewCount=len(reviews),
        counters=counters, groups=groups, scopes=rows, modelDifferences=differences,
        responseIdentityOverlapChecked=True, sourceHistoryCompletenessAttested=False,
        fullInputOutputKnownForExportedScopes=not missing_source and all(
            counters[f]["total"] is not None for f in ("inputTokens", "outputTokens")),
        upstreamExportContentReauthenticated=False, realExecutionAttested=False,
        wholeRunUsageComplete=False, actualCost=None, managementRequestMs=None, wallMs=None,
        experimentRecordReady=False, efficiencyGainProven=False, releaseAccepted=False,
        limitations=[
            "Known subtotals exclude unknown scopes; absent management or reviewed fragments prevent full totals",
            "Provider token units and model differences do not establish comparable cost or efficiency",
            "No prices, provider bill, full timing, task/configuration/authorization binding or real-run attestation",
            "Upstream JSON is hash-selected, not reauthenticated against original journals by this reducer",
        ])


def run(input_path, expected_sha, output_path):
    require(re.fullmatch("[a-f0-9]{64}", expected_sha), "Expected source digest required")
    raw = input_path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == expected_sha, "Source export changed")
    result = analyze(json.loads(raw))
    result["sourceExportSha256"] = expected_sha
    with output_path.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = run(args.input, args.expected_sha256, args.output)
    print(json.dumps({k: result[k] for k in
                      ("recordedScopes", "wholeRunUsageComplete", "experimentRecordReady")}))
