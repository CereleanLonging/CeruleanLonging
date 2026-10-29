"""Read-only audit of the six historical A6API calls in acceptance 286.

This verifies local evidence and reports missing billing inputs. It never
calculates a charge, settles an account, or authorizes a provider request.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path


IDENTIFIER = re.compile(r"^[A-Za-z0-9_.-]{1,200}$")
TOKEN_LIMIT = 100_000_000
MAX_SOURCE_BYTES = 4 * 1024 * 1024
REPORT = Path("reports/a6api-coding-delta-286")


class EvidenceError(ValueError):
    pass


def read_json(path: Path) -> tuple[object, str]:
    if path.stat().st_size > MAX_SOURCE_BYTES:
        raise EvidenceError(f"source exceeds read limit: {path.name}")
    raw = path.read_bytes()
    try:
        return json.loads(raw.decode("utf-8-sig")), hashlib.sha256(raw).hexdigest()
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"source is not UTF-8 JSON: {path.name}") from exc


def nonnegative_token(value: object, label: str) -> int:
    if type(value) is not int or not 0 <= value <= TOKEN_LIMIT:
        raise EvidenceError(f"invalid {label}")
    return value


def usage_fields(value: object) -> dict[str, int | None]:
    if not isinstance(value, dict):
        raise EvidenceError("usage missing")
    usage = {
        "inputTokens": nonnegative_token(value.get("inputTokens"), "inputTokens"),
        "outputTokens": nonnegative_token(value.get("outputTokens"), "outputTokens"),
    }
    for key in ("cacheReadTokens", "cacheWriteTokens"):
        raw = value.get(key)
        usage[key] = None if raw is None else nonnegative_token(raw, key)
        if usage[key] is not None and usage[key] > usage["inputTokens"]:
            raise EvidenceError(f"{key} exceeds reported input")
    read, write = usage["cacheReadTokens"], usage["cacheWriteTokens"]
    if read is not None and write is not None and read + write > usage["inputTokens"]:
        raise EvidenceError("cached input breakdown exceeds reported input")
    return usage


def identifier(value: object, label: str) -> str:
    if not isinstance(value, str) or not IDENTIFIER.fullmatch(value):
        raise EvidenceError(f"invalid {label}")
    return value


def original_path(repo: Path, relative: object, live: Path) -> Path:
    if not isinstance(relative, str) or not relative:
        raise EvidenceError("summary source path missing")
    candidate = (repo / Path(relative.replace("\\", "/"))).resolve(strict=True)
    if not candidate.is_relative_to(live / "data") or not candidate.is_file():
        raise EvidenceError("summary source outside the original live/data tree")
    return candidate


def observed_record(repo: Path, live: Path, summary: object, role: str) -> dict:
    if not isinstance(summary, dict):
        raise EvidenceError("summary record invalid")
    source = original_path(repo, summary.get("path"), live)
    original, digest = read_json(source)
    if digest != summary.get("sha256"):
        raise EvidenceError(f"original source digest mismatch: {source.name}")
    try:
        if role == "management":
            response = original["body"]["evidence"]["response"]
            if original["body"]["evidence"]["kind"] != "deputyResponse":
                raise EvidenceError("management evidence kind changed")
            outcome = response["outcome"]
            if outcome["kind"] != "received":
                raise EvidenceError("management response not received")
            billing = outcome["billing"]
            observed = {
                "responseId": billing["responseId"],
                "actualModel": billing["actualModel"],
                "usage": billing["usage"],
            }
            if billing != summary.get("billing"):
                raise EvidenceError("management summary differs from original")
            purpose = identifier(summary.get("purpose"), "management purpose")
            if purpose not in {"plan", "assign", "accept"}:
                raise EvidenceError("unexpected management purpose")
            operation_id = identifier(summary.get("operationId"), "operation ID")
            if response["request"]["handoff"]["operationId"] != operation_id:
                raise EvidenceError("management operation ID mismatch")
        else:
            event = original["body"]["event"]
            if event["kind"] != "outcome" or event["outcome"]["kind"] != "reply":
                raise EvidenceError("worker response not a reply")
            observation = event["outcome"]["observation"]
            observed = {
                "responseId": observation["responseId"],
                "actualModel": observation["actualModel"],
                "usage": observation["usage"],
            }
            if any(observed[key] != summary.get(key) for key in observed):
                raise EvidenceError("worker summary differs from original")
            purpose = "worker"
            operation_id = None
    except (KeyError, TypeError, IndexError) as exc:
        raise EvidenceError("original source lacks expected reply fields") from exc
    return {
        "role": role,
        "purpose": purpose,
        "operationId": operation_id,
        "responseId": identifier(observed["responseId"], "response ID"),
        "actualModel": identifier(observed["actualModel"], "actual model"),
        "usage": usage_fields(observed["usage"]),
        "source": str(source.relative_to(repo)).replace("\\", "/"),
        "sourceSha256": digest,
        "actualMoneyUsdNanos": None,
    }


def audit(repo_root: Path) -> dict:
    repo = repo_root.resolve(strict=True)
    live = (repo / REPORT / "live").resolve(strict=True)
    latest, latest_sha = read_json(repo / REPORT / "latest.json")
    management, management_sha = read_json(live / "management-summary.json")
    worker, worker_sha = read_json(live / "worker-summary.json")
    if not isinstance(latest, dict) or latest.get("stage") != 286 or latest.get("provider") != "a6api":
        raise EvidenceError("this audit is bound to A6API acceptance 286")
    if latest.get("actualMoney") is not None:
        raise EvidenceError("286 money state changed; inspect before using this audit")
    if not isinstance(management, list) or not isinstance(worker, list):
        raise EvidenceError("summary lists missing")
    records = [observed_record(repo, live, item, "management") for item in management]
    records.extend(observed_record(repo, live, item, "worker") for item in worker)
    response_ids = [item["responseId"] for item in records]
    sources = [item["source"] for item in records]
    if len(set(response_ids)) != len(response_ids) or len(set(sources)) != len(sources):
        raise EvidenceError("duplicate response ID or source")
    if (
        len(management) != latest.get("managementRequests")
        or len(worker) != latest.get("workerRequests")
        or len(records) != latest.get("totalProviderRequests")
        or sorted(item["purpose"] for item in records if item["role"] == "management")
        != ["accept", "assign", "plan"]
    ):
        raise EvidenceError("request inventory differs from 286 result")
    aggregation = {
        "inputTokens": sum(item["usage"]["inputTokens"] for item in records),
        "outputTokens": sum(item["usage"]["outputTokens"] for item in records),
    }
    for key in ("cacheReadTokens", "cacheWriteTokens"):
        values = [item["usage"][key] for item in records]
        missing = sum(value is None for value in values)
        aggregation[key] = {
            "reportedSum": sum(value for value in values if value is not None),
            "missingResponses": missing,
            "complete": missing == 0,
        }
    if aggregation != latest.get("usage"):
        raise EvidenceError("recomputed usage differs from 286 result")
    return {
        "schema": 1,
        "scope": "a6api-286-six-replies-only",
        "provider": "a6api",
        "endpoint": latest.get("endpoint"),
        "requestedModel": latest.get("requestedModel"),
        "localReplySummaryIntegrityVerified": True,
        "providerBillingAuthenticityVerified": False,
        "requestCount": len(records),
        "usage": aggregation,
        "completeCacheBreakdown": all(
            aggregation[key]["complete"] for key in ("cacheReadTokens", "cacheWriteTokens")
        ),
        "externalBillingEvidenceProvided": False,
        "actualMoneyUsdNanos": None,
        "executionPermissionGranted": False,
        "sourceSha256": {
            "latest": latest_sha,
            "managementSummary": management_sha,
            "workerSummary": worker_sha,
        },
        "records": records,
        "awaitingBillingMatchResponseIds": response_ids,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        result = audit(args.repo_root)
    except (EvidenceError, OSError) as exc:
        print(f"money-readiness audit failed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
