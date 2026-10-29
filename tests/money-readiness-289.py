"""Evidence integrity and fail-closed checks for the read-only 289 audit."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
SCRIPT = PROJECT / "scripts" / "money-readiness-289.py"
spec = importlib.util.spec_from_file_location("money_readiness_289", SCRIPT)
audit_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit_module)


def write_json(path: Path, value: object) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    path.write_bytes(raw)
    return hashlib.sha256(raw).hexdigest()


class ReadinessChecks(unittest.TestCase):
    def setUp(self):
        parent = PROJECT / "reports" / "money-readiness-289"
        parent.mkdir(parents=True, exist_ok=True)
        self.scratch = tempfile.TemporaryDirectory(dir=parent)
        self.repo = Path(self.scratch.name).resolve()
        self.assertTrue(self.repo.is_relative_to(parent.resolve()))
        self.report = self.repo / "reports" / "a6api-coding-delta-286"
        self.live = self.report / "live"
        management = []
        worker = []
        self.sources = []
        tokens = [
            (10, 2, None, None),
            (20, 3, None, None),
            (30, 4, None, None),
            (40, 5, None, None),
            (50, 6, 10, None),
            (60, 7, 20, None),
        ]
        for index, (input_tokens, output_tokens, read, write) in enumerate(tokens):
            usage = {
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "cacheReadTokens": read,
                "cacheWriteTokens": write,
            }
            billing = {
                "responseId": f"resp_{index}",
                "actualModel": "gpt-6.1-sol",
                "usage": usage,
            }
            if index < 3:
                purpose = ("plan", "assign", "accept")[index]
                operation = f"operation-{purpose}"
                original = {
                    "body": {
                        "evidence": {
                            "kind": "deputyResponse",
                            "response": {
                                "request": {"handoff": {"operationId": operation}},
                                "outcome": {"kind": "received", "billing": billing},
                            },
                        }
                    }
                }
                relative = f"reports/a6api-coding-delta-286/live/data/management/{index}.json"
                path = self.repo / relative
                digest = write_json(path, original)
                management.append({
                    "path": relative,
                    "sha256": digest,
                    "purpose": purpose,
                    "operationId": operation,
                    "billing": billing,
                })
            else:
                original = {
                    "body": {
                        "event": {
                            "kind": "outcome",
                            "outcome": {"kind": "reply", "observation": billing},
                        }
                    }
                }
                relative = f"reports/a6api-coding-delta-286/live/data/worker/{index}.json"
                path = self.repo / relative
                digest = write_json(path, original)
                worker.append({"path": relative, "sha256": digest, **billing})
            self.sources.append(path)
        self.management = management
        self.worker = worker
        self.latest = {
            "stage": 286,
            "provider": "a6api",
            "endpoint": "https://api.a6api.com/v1/chat/completions",
            "requestedModel": "gpt-6.1-sol",
            "managementRequests": 3,
            "workerRequests": 3,
            "totalProviderRequests": 6,
            "actualMoney": None,
            "usage": {
                "inputTokens": 210,
                "outputTokens": 27,
                "cacheReadTokens": {"reportedSum": 30, "missingResponses": 4, "complete": False},
                "cacheWriteTokens": {"reportedSum": 0, "missingResponses": 6, "complete": False},
            },
        }
        self.save()

    def tearDown(self):
        self.assertTrue(self.repo.is_relative_to((PROJECT / "reports" / "money-readiness-289").resolve()))
        self.scratch.cleanup()

    def save(self):
        write_json(self.report / "latest.json", self.latest)
        write_json(self.live / "management-summary.json", self.management)
        write_json(self.live / "worker-summary.json", self.worker)

    def test_real_286_six_original_replies_and_missing_billing(self):
        result = audit_module.audit(PROJECT)
        self.assertEqual(result["requestCount"], 6)
        self.assertEqual(result["usage"]["inputTokens"], 43735)
        self.assertEqual(result["usage"]["outputTokens"], 951)
        self.assertEqual(result["usage"]["cacheReadTokens"]["missingResponses"], 4)
        self.assertEqual(result["usage"]["cacheWriteTokens"]["missingResponses"], 6)
        self.assertIsNone(result["actualMoneyUsdNanos"])
        self.assertFalse(result["executionPermissionGranted"])

    def test_control_fixture_and_tampered_source(self):
        self.assertEqual(audit_module.audit(self.repo)["requestCount"], 6)
        self.sources[0].write_bytes(self.sources[0].read_bytes() + b" ")
        with self.assertRaisesRegex(audit_module.EvidenceError, "digest mismatch"):
            audit_module.audit(self.repo)

    def test_summary_cannot_fill_unknown_cache_with_zero(self):
        self.management[0]["billing"]["usage"]["cacheReadTokens"] = 0
        self.save()
        with self.assertRaisesRegex(audit_module.EvidenceError, "summary differs"):
            audit_module.audit(self.repo)

    def test_duplicate_response_identity_is_rejected(self):
        raw = json.loads(self.sources[4].read_text(encoding="utf-8"))
        raw["body"]["event"]["outcome"]["observation"]["responseId"] = "resp_3"
        self.worker[1]["responseId"] = "resp_3"
        self.worker[1]["sha256"] = write_json(self.sources[4], raw)
        self.save()
        with self.assertRaisesRegex(audit_module.EvidenceError, "duplicate response ID"):
            audit_module.audit(self.repo)

    def test_amount_change_requires_new_review(self):
        self.latest["actualMoney"] = 0
        self.save()
        with self.assertRaisesRegex(audit_module.EvidenceError, "money state changed"):
            audit_module.audit(self.repo)

    def test_boolean_and_overlapping_cache_counters_rejected(self):
        with self.assertRaises(audit_module.EvidenceError):
            audit_module.usage_fields({
                "inputTokens": True, "outputTokens": 1,
                "cacheReadTokens": None, "cacheWriteTokens": None,
            })
        with self.assertRaisesRegex(audit_module.EvidenceError, "breakdown exceeds"):
            audit_module.usage_fields({
                "inputTokens": 10, "outputTokens": 1,
                "cacheReadTokens": 6, "cacheWriteTokens": 5,
            })

    def test_summary_cannot_escape_live_data(self):
        self.management[0]["path"] = "../elsewhere.json"
        self.save()
        with self.assertRaises((audit_module.EvidenceError, OSError)):
            audit_module.audit(self.repo)


if __name__ == "__main__":
    unittest.main()
