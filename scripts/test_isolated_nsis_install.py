"""No installer or application is launched by these checks."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import isolated_nsis_install as driver

CURRENT_RESOURCES = ("SQLITE-NOTICES.txt", "THIRD-PARTY-NOTICES.txt",
                     "RUST-LIBRARY-COPYRIGHT.html", "THIRD-PARTY-SOURCES.zip",
                     "THIRD-PARTY-MANIFEST.json")


class IsolationChecks(unittest.TestCase):
    def observation_fixture(self, folder, first_error=None):
        root = Path(folder)
        (root / "reports").mkdir()
        package, _, sha = self.fixture(folder, "0.1.22")
        run = root / "reports" / "isolated-nsis-observation"
        args = argparse.Namespace(package=str(package), manifest_sha256=sha,
                                  run_dir=str(run), dry_run=False, timeout=1)

        class ControlledProcess:
            pid, _handle, returncode = 333, 444, None
            observations = []

            def communicate(self, timeout=None):
                self.observations.append(timeout)
                if first_error is not None and len(self.observations) == 1:
                    raise first_error
                self.returncode = 0
                return b"observed stdout", b"observed stderr"

            def kill(self):
                raise AssertionError("Must not kill the original child")

            terminate = kill

        return root, run, args, ControlledProcess()

    def test_post_launch_receipt_failure_observes_original_and_never_passes(self):
        for name in ("original-process.json", "timeout-observation.json", "finished.json"):
            with self.subTest(receipt=name), tempfile.TemporaryDirectory() as folder:
                first_error = (subprocess.TimeoutExpired("controlled", 1)
                               if name == "timeout-observation.json" else None)
                root, run, args, process = self.observation_fixture(folder, first_error)
                save = driver.save_new

                def failed_receipt(path, value):
                    if Path(path).name == name:
                        raise OSError("controlled disk failure")
                    save(path, value)

                with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]) as registry, patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=process) as launch, patch.object(driver, "save_new", side_effect=failed_receipt), patch.object(driver, "validate_installed") as validate:
                    result = driver.execute(args)
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(registry.call_count, 2)
                validate.assert_not_called()
                self.assertEqual(process.observations, [1, None] if first_error else [1])
                self.assertEqual(result["status"], "observation-evidence-failed")
                self.assertEqual(result["installerExitCode"], 0)
                self.assertTrue(result["originalProcessObserved"])
                self.assertTrue(result["stdoutEOF"] and result["stderrEOF"])
                self.assertEqual(result["observationErrors"], [{"stage": name, "type": "OSError"}])
                self.assertNotIn("resourcesMatch", result)
                self.assertEqual((run / "installer.stdout").read_bytes(), b"observed stdout")
                self.assertEqual((run / "installer.stderr").read_bytes(), b"observed stderr")
                self.assertEqual(json.loads((run / "result.json").read_text(encoding="utf-8")), result)

    def test_log_failure_preserves_other_stream_and_terminal_evidence(self):
        for name in ("installer.stdout", "installer.stderr"):
            with self.subTest(log=name), tempfile.TemporaryDirectory() as folder:
                root, run, args, process = self.observation_fixture(folder)
                open_path = Path.open

                def failed_log(path, *args, **kwargs):
                    if path.name == name and args and args[0] == "xb":
                        raise OSError("controlled log failure")
                    return open_path(path, *args, **kwargs)

                with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=process) as launch, patch.object(Path, "open", failed_log):
                    result = driver.execute(args)
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(result["status"], "observation-evidence-failed")
                other = "installer.stderr" if name.endswith("stdout") else "installer.stdout"
                self.assertTrue((run / other).is_file())
                finished = json.loads((run / "finished.json").read_text(encoding="utf-8"))
                self.assertEqual(finished["observationErrors"], [{"stage": name, "type": "OSError"}])
                self.assertEqual(finished["exit"], 0)
                self.assertTrue(finished["stdoutEOF"] and finished["stderrEOF"])

    def test_timeout_notification_failure_keeps_original_observation(self):
        with tempfile.TemporaryDirectory() as folder:
            root, run, args, process = self.observation_fixture(folder, subprocess.TimeoutExpired("controlled", 1))
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=process) as launch, patch("builtins.print", side_effect=BrokenPipeError("controlled observer disconnect")):
                result = driver.execute(args)
            self.assertEqual(launch.call_count, 1)
            self.assertEqual(process.observations, [1, None])
            self.assertEqual(result["status"], "observation-evidence-failed")
            self.assertEqual(result["observationErrors"], [{"stage": "timeout-notification", "type": "BrokenPipeError"}])
            self.assertTrue((run / "finished.json").is_file())

    def test_keyboard_interrupt_waits_original_before_reporting_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            root, run, args, process = self.observation_fixture(folder, KeyboardInterrupt())
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=process) as launch:
                result = driver.execute(args)
            self.assertEqual(launch.call_count, 1)
            self.assertEqual(process.observations, [1, None])
            self.assertEqual(result["status"], "observation-evidence-failed")
            self.assertEqual(result["observationErrors"], [{"stage": "communicate", "type": "KeyboardInterrupt"}])
            self.assertFalse(result["initialTimeout"])

    def test_intent_write_failure_prevents_launch(self):
        with tempfile.TemporaryDirectory() as folder:
            root, run, args, _ = self.observation_fixture(folder)
            save = driver.save_new

            def failed_intent(path, value):
                if Path(path).name == "startup-intent.json":
                    raise OSError("controlled intent failure")
                save(path, value)

            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen") as launch, patch.object(driver, "save_new", side_effect=failed_intent):
                result = driver.execute(args)
            launch.assert_not_called()
            self.assertEqual(result["installerLaunches"], 0)
            self.assertEqual(result["status"], "preflight-failed")

    def test_real_harmless_child_drains_both_pipes_after_receipt_failure(self):
        # This runs only Python writing constants; no installer, product, or
        # registry access. Each stream exceeds the Windows anonymous pipe size.
        with tempfile.TemporaryDirectory() as folder:
            run = Path(folder)
            child = subprocess.Popen([sys.executable, "-I", "-S", "-c",
                "import os; os.write(1,b'o'*524288); os.write(2,b'e'*524288)"],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                env={key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR") if key in os.environ},
                creationflags=subprocess.CREATE_NO_WINDOW)
            save = driver.save_new

            def failed_receipt(path, value):
                if Path(path).name == "original-process.json":
                    raise OSError("controlled receipt failure with live child")
                save(path, value)

            result = {}
            try:
                with patch.object(driver, "save_new", side_effect=failed_receipt):
                    with self.assertRaisesRegex(RuntimeError, "Original installer observed"):
                        driver.observe_installer(child, run, 5, result)
            finally:
                # An unexpected test failure still owns this harmless child.
                # Cleanup cannot create the driver's missing logs or success.
                if child.poll() is None:
                    child.communicate()
            self.assertEqual(child.poll(), 0)
            self.assertTrue(child.stdout.closed and child.stderr.closed)
            self.assertEqual((run / "installer.stdout").read_bytes(), b"o" * 524288)
            self.assertEqual((run / "installer.stderr").read_bytes(), b"e" * 524288)
            self.assertTrue(result["originalProcessObserved"])
            self.assertEqual(result["status"], "observation-evidence-failed")

    def test_raw_suffix_preserves_spaces_without_argv_quotes(self):
        exe = r"D:\package folder\setup.exe"
        dest = r"D:\test\reports\isolated-nsis-check\installed app"
        raw = driver.raw_nsis_command(exe, dest)
        self.assertEqual(raw, '"' + exe + '" /S /NS /D=' + dest)
        self.assertNotIn('"/D=', raw)
        self.assertTrue(raw.endswith(dest))
        self.assertIn('"/D=', subprocess.list2cmdline([exe, "/S", "/NS", "/D=" + dest]))

    def test_command_rejects_injected_quotes_controls_and_nonlocal_paths(self):
        for bad in ('D:\\x" /R', 'D:\\x\r\n/R', 'D:\\x\x00', 'D:\\x\x7f', r"\\server\share", r"relative\path"):
            with self.subTest(path=bad), self.assertRaises(ValueError):
                driver.raw_nsis_command(r"D:\safe\setup.exe", bad)

    def test_destination_requires_new_owned_direct_child(self):
        with tempfile.TemporaryDirectory() as folder:
            reports = Path(folder) / "reports"
            reports.mkdir()
            good = reports / "isolated-nsis-new"
            self.assertEqual(driver.owned_run(reports, good), good)
            for bad in (reports, reports.parent / "isolated-nsis-out", reports / "old", reports / "sub" / "isolated-nsis-nested", reports / ("isolated-nsis-" + "a" * 230)):
                with self.subTest(path=bad), self.assertRaises(ValueError):
                    driver.owned_run(reports, bad)
            good.mkdir()
            with self.assertRaises(ValueError):
                driver.owned_run(reports, good)

    def test_existing_registration_refused_in_any_scope(self):
        driver.require_no_registration([{"present": False}])
        for hive in ("HKCU", "HKLM"):
            for view in ("32", "64"):
                with self.subTest(hive=hive, view=view), self.assertRaises(ValueError):
                    driver.require_no_registration([{"present": True, "hive": hive, "view": view}])

    def fixture(self, folder, version="0.1.7"):
        package = Path(folder) / "package"
        package.mkdir()
        artifacts = []
        for name in ("setup.exe", *driver.PAYLOADS):
            data = (name + " fixture, never executable").encode()
            (package / name).write_bytes(data)
            artifacts.append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        manifest = {"product": "CeruleanLonging", "identifier": "local.ceruleanlonging.workbench", "version": version, "artifacts": artifacts}
        if version in ("0.1.21", "0.1.22"):
            manifest["resources"] = []
            for name in CURRENT_RESOURCES if version == "0.1.22" else CURRENT_RESOURCES[:1]:
                data = ("notice fixture: " + name).encode()
                (package / name).write_bytes(data)
                manifest["resources"].append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        return package, manifest, self.save_manifest(package, manifest)

    def save_manifest(self, package, manifest):
        (package / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        return driver.digest(package / "manifest.json")

    def registration(self, install, version="0.1.7"):
        return [{"present": True, "hive": "HKCU", "view": "64", "key": driver.PRODUCT_KEY,
                 "values": {"": {"value": str(install)}}},
                {"present": True, "hive": "HKCU", "view": "64", "key": driver.UNINSTALL_KEY,
                 "values": {k: {"value": v} for k, v in {"DisplayVersion": version,
                     "InstallLocation": '"' + str(install) + '"',
                     "UninstallString": '"' + str(install / "uninstall.exe") + '"'}.items()}}]

    def test_manifest_and_payload_changes_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            package, manifest, sha = self.fixture(folder)
            self.assertEqual(driver.inspect_package(package, sha)[1], manifest)
            with self.assertRaises(ValueError):
                driver.inspect_package(package, "0" * 64)
            (package / "setup.exe").write_bytes(b"changed")
            with self.assertRaises(ValueError):
                driver.inspect_package(package, sha)

    def test_installed_version_path_and_bytes_all_required(self):
        with tempfile.TemporaryDirectory() as folder:
            install, manifest, _ = self.fixture(folder)
            (install / "uninstall.exe").write_bytes(b"uninstaller fixture")
            driver.validate_installed(install, manifest, self.registration(install))
            for registration in ([], self.registration(install, "0.1.8"), self.registration(install.parent)):
                with self.subTest(registration=registration), self.assertRaises(ValueError):
                    driver.validate_installed(install, manifest, registration)
            (install / "cerulean-runtime.exe").write_bytes(b"changed")
            with self.assertRaises(ValueError):
                driver.validate_installed(install, manifest, self.registration(install))

    def test_resource_versions_accept_original_and_current_layouts(self):
        for version, count in (("0.1.7", 0), ("0.1.21", 1), ("0.1.22", 5)):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as folder:
                package, manifest, sha = self.fixture(folder, version)
                self.assertEqual(driver.inspect_package(package, sha)[1], manifest)
                self.assertEqual(len(driver.package_resources(manifest)), count)
                (package / "uninstall.exe").write_bytes(b"uninstaller fixture")
                driver.validate_installed(package, manifest, self.registration(package, version))

    def test_bad_resource_sets_refused_before_any_launch(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            package, manifest, _ = self.fixture(folder, "0.1.22")
            good = manifest["resources"]
            variants = {"omitted": None, "empty": [], "object": {}, "null": None,
                        "non-object-entry": [None] * 5, "missing": good[:-1],
                        "duplicate": good[:-1] + [good[0]], "extra": good + [good[0]],
                        "traversal": [dict(good[0], path="../SQLITE-NOTICES.txt"), *good[1:]],
                        "absolute": [dict(good[0], path=str(root / "SQLITE-NOTICES.txt")), *good[1:]],
                        "case": [dict(good[0], path="sqlite-notices.txt"), *good[1:]],
                        "non-text-name": [dict(good[0], path=[]), *good[1:]]}
            for label, resources in variants.items():
                with self.subTest(label=label):
                    candidate = dict(manifest)
                    if label == "omitted":
                        candidate.pop("resources")
                    else:
                        candidate["resources"] = resources
                    sha = self.save_manifest(package, candidate)
                    run = root / "reports" / ("isolated-nsis-" + label)
                    args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=False, timeout=1)
                    with patch.object(driver, "ROOT", root), patch.object(driver.subprocess, "Popen") as launch:
                        with self.assertRaises(ValueError):
                            driver.execute(args)
                        launch.assert_not_called()
                        self.assertFalse(run.exists())

    def test_resource_size_and_digest_declarations_are_bounded(self):
        with tempfile.TemporaryDirectory() as folder:
            package, manifest, _ = self.fixture(folder, "0.1.22")
            good = manifest["resources"]
            changes = [{"bytes": value} for value in (0, -1, True, 1.5, "24", 1024 * 1024 + 1)]
            changes += [{"sha256": value} for value in (None, "x" * 64, "0" * 63, "A" * 64)]
            for change in changes:
                with self.subTest(change=change):
                    candidate = dict(manifest, resources=[dict(good[0], **change), *good[1:]])
                    sha = self.save_manifest(package, candidate)
                    with self.assertRaisesRegex(ValueError, "size or digest"):
                        driver.inspect_package(package, sha)

    def test_sqlite_notice_cannot_be_omitted_at_021(self):
        with tempfile.TemporaryDirectory() as folder:
            package, manifest, _ = self.fixture(folder, "0.1.21")
            manifest.pop("resources")
            sha = self.save_manifest(package, manifest)
            with self.assertRaisesRegex(ValueError, "Required notice resource"):
                driver.inspect_package(package, sha)

    def test_every_packaged_resource_missing_or_tampered_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            package, _, sha = self.fixture(folder, "0.1.22")
            for name in CURRENT_RESOURCES:
                path = package / name
                original = path.read_bytes()
                for failure in ("missing", "same-size", "length"):
                    with self.subTest(name=name, failure=failure):
                        if failure == "missing":
                            path.unlink()
                        else:
                            path.write_bytes(b"x" * (len(original) + (failure == "length")))
                        with self.assertRaisesRegex(ValueError, "Notice resource"):
                            driver.inspect_package(package, sha)
                        path.write_bytes(original)

    def test_every_installed_resource_is_required_even_when_registration_matches(self):
        with tempfile.TemporaryDirectory() as folder:
            install, manifest, _ = self.fixture(folder, "0.1.22")
            (install / "uninstall.exe").write_bytes(b"uninstaller fixture")
            registration = self.registration(install, "0.1.22")
            for name in CURRENT_RESOURCES:
                path = install / name
                original = path.read_bytes()
                for failure in ("missing", "same-size", "length"):
                    with self.subTest(name=name, failure=failure):
                        if failure == "missing":
                            path.unlink()
                        else:
                            path.write_bytes(b"x" * (len(original) + (failure == "length")))
                        with self.assertRaisesRegex(ValueError, "Notice resource"):
                            driver.validate_installed(install, manifest, registration)
                        path.write_bytes(original)
            driver.validate_installed(install, manifest, registration)

    def test_manifest_read_is_bounded_and_bom_remains_supported(self):
        with tempfile.TemporaryDirectory() as folder:
            package, manifest, _ = self.fixture(folder, "0.1.22")
            path = package / "manifest.json"
            path.write_text(json.dumps(manifest), encoding="utf-8-sig")
            self.assertEqual(driver.inspect_package(package, driver.digest(path))[1], manifest)
            path.write_bytes(b" " * (1024 * 1024 + 1))
            with self.assertRaisesRegex(ValueError, "size bound"):
                driver.inspect_package(package, driver.digest(path))

    def test_current_resource_dry_run_records_declarations_without_installing(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            package, manifest, sha = self.fixture(folder, "0.1.22")
            run = root / "reports" / "isolated-nsis-current-dry"
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=True, timeout=1)
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent-default"), patch.object(driver.subprocess, "Popen") as launch:
                result = driver.execute(args)
                launch.assert_not_called()
                self.assertEqual(result["status"], "dry-run-ready-no-installation")
                request = json.loads((run / "request.json").read_text(encoding="utf-8"))
                self.assertEqual(request["resources"], manifest["resources"])
                self.assertNotIn("resourcesMatch", result)

    def test_current_install_result_requires_resources_and_never_retries(self):
        for failure in (None, "missing", "tampered"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                (root / "reports").mkdir()
                package, _, sha = self.fixture(folder, "0.1.22")
                run = root / "reports" / "isolated-nsis-current"
                install = run / "installed app"
                args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=False, timeout=1)
                class ControlledProcess:
                    pid, _handle, returncode = 123, 456, 0
                    def communicate(self, timeout=None):
                        install.mkdir()
                        for name in (*driver.PAYLOADS, *CURRENT_RESOURCES):
                            (install / name).write_bytes((package / name).read_bytes())
                        (install / "uninstall.exe").write_bytes(b"fixture")
                        if failure == "missing":
                            (install / "THIRD-PARTY-SOURCES.zip").unlink()
                        elif failure == "tampered":
                            (install / "THIRD-PARTY-SOURCES.zip").write_bytes(b"changed")
                        return b"controlled fixture", b""
                with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", side_effect=[[], [], self.registration(install, "0.1.22")]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=ControlledProcess()) as launch:
                    result = driver.execute(args)
                    self.assertEqual(launch.call_count, 1)
                    self.assertFalse(result["automaticRetry"])
                    self.assertFalse(result["cleanupAttempted"])
                    if failure is None:
                        self.assertEqual(result["status"], "passed")
                        self.assertTrue(result["resourcesMatch"])
                        self.assertEqual(result["verifiedResourceCount"], 5)
                    else:
                        self.assertEqual(result["status"], "post-install-verification-failed")
                        self.assertNotIn("resourcesMatch", result)
                        self.assertIn("THIRD-PARTY-SOURCES.zip", result["error"])
                    self.assertEqual(json.loads((run / "result.json").read_text(encoding="utf-8")), result)
                    self.assertTrue((install / "uninstall.exe").is_file())

    def test_failure_does_not_launch_or_overwrite_existing_evidence(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            package, _, sha = self.fixture(folder)
            run = root / "reports" / "isolated-nsis-check"
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=False, timeout=1)
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[{"present": True}]), patch.object(driver.subprocess, "Popen") as launch:
                result = driver.execute(args)
                launch.assert_not_called()
                self.assertEqual(result["status"], "preflight-failed")
                self.assertEqual(result["installerLaunches"], 0)
                original = (run / "result.json").read_bytes()
                with self.assertRaises(ValueError):
                    driver.execute(args)
                self.assertEqual((run / "result.json").read_bytes(), original)

    def test_dry_run_never_launches_even_when_ready(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            package, _, sha = self.fixture(folder)
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(root / "reports" / "isolated-nsis-dry"), dry_run=True, timeout=1)
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent-default"), patch.object(driver.subprocess, "Popen") as launch:
                result = driver.execute(args)
                launch.assert_not_called()
                self.assertEqual(result["status"], "dry-run-ready-no-installation")
                self.assertEqual(result["installerLaunches"], 0)

    def test_default_install_without_registration_also_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            (root / "CeruleanLonging").mkdir()
            package, _, sha = self.fixture(folder)
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(root / "reports" / "isolated-nsis-default"), dry_run=False, timeout=1)
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root), patch.object(driver.subprocess, "Popen") as launch:
                result = driver.execute(args)
                launch.assert_not_called()
                self.assertIn("Unregistered default installation", result["error"])

    def test_ambiguous_startup_is_preserved_without_retry(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            package, _, sha = self.fixture(folder)
            run = root / "reports" / "isolated-nsis-unknown"
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=False, timeout=1)
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", return_value=[]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", side_effect=OSError("controlled startup error")) as launch:
                result = driver.execute(args)
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(result["status"], "startup-unknown")
                self.assertEqual(result["launchAttempts"], 1)
                self.assertIsNone(result["installerLaunches"])
                self.assertTrue((run / "startup-intent.json").is_file())
                self.assertIsInstance(launch.call_args.args[0], str)
                self.assertFalse(launch.call_args.kwargs["shell"])

    def test_timeout_observes_same_handle_without_second_launch(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "reports").mkdir()
            package, _, sha = self.fixture(folder, "0.1.22")
            run = root / "reports" / "isolated-nsis-timeout"
            install = run / "installed app"
            args = argparse.Namespace(package=str(package), manifest_sha256=sha, run_dir=str(run), dry_run=False, timeout=1)
            class ControlledProcess:
                pid, _handle, returncode, observations = 123, 456, 0, 0
                def communicate(self, timeout=None):
                    self.observations += 1
                    if self.observations == 1:
                        raise subprocess.TimeoutExpired("controlled fixture", timeout)
                    install.mkdir()
                    for name in (*driver.PAYLOADS, *CURRENT_RESOURCES):
                        (install / name).write_bytes((package / name).read_bytes())
                    (install / "uninstall.exe").write_bytes(b"fixture")
                    return b"controlled stdout", b"controlled stderr"
            process = ControlledProcess()
            with patch.object(driver, "ROOT", root), patch.object(driver, "registry_snapshot", side_effect=[[], [], self.registration(install, "0.1.22")]), patch.object(driver, "known_folder", return_value=root / "absent"), patch.object(driver.subprocess, "Popen", return_value=process) as launch:
                result = driver.execute(args)
                self.assertEqual(launch.call_count, 1)
                self.assertEqual(process.observations, 2)
                self.assertEqual(result["status"], "verified-after-timeout-requires-review")
                self.assertTrue(result["resourcesMatch"])
                self.assertEqual(result["verifiedResourceCount"], 5)
                finished = json.loads((run / "finished.json").read_text(encoding="utf-8"))
                self.assertTrue(finished["initialTimeout"])
                self.assertTrue(finished["waitedOriginalHandle"])
                self.assertTrue(finished["stdoutEOF"])
                self.assertTrue(finished["stderrEOF"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
