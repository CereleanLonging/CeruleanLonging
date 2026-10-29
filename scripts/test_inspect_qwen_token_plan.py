import json
from pathlib import Path
import tempfile
import unittest

from inspect_qwen_token_plan import ENDPOINT, KEY_ENV, MAX_BYTES, MODEL, inspect_settings


class QwenInspectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "settings.json"
        self.secret = "synthetic-credential-never-print"
        self.config = {
            "env": {KEY_ENV: self.secret},
            "modelProviders": {"openai": [{"id": MODEL, "baseUrl": ENDPOINT, "envKey": KEY_ENV}]},
            "model": {"name": MODEL},
            "security": {"auth": {"selectedType": "openai"}},
        }

    def inspect(self):
        self.path.write_text(json.dumps(self.config), encoding="utf-8")
        before = self.path.read_bytes()
        result = inspect_settings(self.path, {})
        self.assertEqual(before, self.path.read_bytes())
        self.assertNotIn(self.secret, json.dumps(result))
        return result

    def test_presence_is_not_authentication_or_effective_config(self):
        result = self.inspect()
        self.assertEqual("matchingProfile", result["status"])
        self.assertEqual("inlinePresent", result["credentialPresence"])
        self.assertEqual("notTested", result["credentialValidity"])
        self.assertEqual("notEvaluated", result["effectiveMergedConfiguration"])
        self.assertEqual(0, result["realProviderCalls"])

    def test_wrong_endpoint_or_duplicate_profile_cannot_pass(self):
        profile = self.config["modelProviders"]["openai"][0]
        profile["baseUrl"] = "https://coding.dashscope.aliyuncs.com/v1"
        self.assertEqual("needsReview", self.inspect()["status"])
        profile["baseUrl"] = ENDPOINT
        self.config["modelProviders"]["openai"].append(dict(profile))
        self.assertEqual("needsReview", self.inspect()["status"])

    def test_invalid_or_duplicate_json_does_not_leak(self):
        for raw in [self.secret, '{"env":{},"env":{"secret":"' + self.secret + '"}}', '[]']:
            self.path.write_text(raw, encoding="utf-8")
            result = inspect_settings(self.path, {})
            self.assertEqual("invalidConfiguration", result["status"])
            self.assertNotIn(self.secret, json.dumps(result))

    def test_no_arbitrary_environment_lookup_and_bounded_read(self):
        self.config["modelProviders"]["openai"][0]["envKey"] = "OTHER_SECRET"
        self.config["env"] = {}
        self.assertEqual("needsReview", self.inspect()["status"])
        result = inspect_settings(self.path, {"OTHER_SECRET": self.secret})
        self.assertEqual("notObserved", result["credentialPresence"])
        self.path.write_bytes(b"x" * (MAX_BYTES + 1))
        self.assertEqual("oversize", inspect_settings(self.path, {})["status"])


if __name__ == "__main__":
    unittest.main()
