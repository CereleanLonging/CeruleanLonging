"""Read-only Qwen configuration check. Never emit configuration values/secrets."""
import argparse
import json
import os
from pathlib import Path

MODEL = "qwen3.8-flash"
ENDPOINT = "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
KEY_ENV = "BAILIAN_TOKEN_PLAN_API_KEY"
MAX_BYTES = 1024 * 1024


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate field")
        result[key] = value
    return result


def inspect_settings(path, environ=None):
    """Return fixed labels only; does not resolve arbitrary config envKey names."""
    environ = os.environ if environ is None else environ
    path = Path(path)
    if path.is_dir():
        path = path / "settings.json"
    try:
        with path.open("rb") as stream:
            raw = stream.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            return {"status": "oversize", "realProviderCalls": 0}
        config = json.loads(raw.decode("utf-8-sig"), object_pairs_hook=unique_object)
        profiles = config.get("modelProviders", {}).get("openai", [])
        if not isinstance(profiles, list) or not all(isinstance(p, dict) for p in profiles):
            raise ValueError("invalid profiles")
        selected = [p for p in profiles if p.get("id") == MODEL]
        matches = [
            p for p in selected
            if p.get("baseUrl") == ENDPOINT and p.get("envKey") == KEY_ENV
        ]
        inline = config.get("env", {}).get(KEY_ENV)
        process = environ.get(KEY_ENV)
        # Values never leave this process, including failures and diagnostics.
        present = lambda value: isinstance(value, str) and bool(value.strip()) and value != "YOUR_API_KEY"
        credential = (
            "processEnvironmentPresent" if present(process)
            else "inlinePresent" if present(inline) else "notObserved"
        )
        return {
            "status": "matchingProfile" if len(selected) == len(matches) == 1 else "needsReview",
            "model": MODEL,
            "matchingProfiles": len(matches),
            "requestedModelProfiles": len(selected),
            "defaultModelMatches": config.get("model", {}).get("name") == MODEL,
            "authTypeMatches": config.get("security", {}).get("auth", {}).get("selectedType") == "openai",
            "credentialPresence": credential,
            "credentialValidity": "notTested",
            "effectiveMergedConfiguration": "notEvaluated",
            "realProviderCalls": 0,
        }
    except OSError:
        return {"status": "unreadable", "realProviderCalls": 0}
    except (ValueError, TypeError, AttributeError, RecursionError):
        return {"status": "invalidConfiguration", "realProviderCalls": 0}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("settings", type=Path, help="Explicitly selected settings.json or its directory")
    args = parser.parse_args()
    result = inspect_settings(args.settings)
    print(json.dumps(result, ensure_ascii=True, indent=2))
    raise SystemExit(0 if result["status"] == "matchingProfile" else 2)
