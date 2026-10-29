"""Small isolated input-drift checks; compiler probes are mocked, never executed."""
from pathlib import Path
import copy
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('distribution_build_inputs', HERE / 'distribution-build-inputs.py')
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class BuildInputChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='cerulean-build-inputs-319-')
        self.addCleanup(self.temp.cleanup)
        base = Path(self.temp.name)
        self.root = base / 'repository'
        self.cargo = base / 'cargo'
        self.rustup = base / 'rustup'
        self.sysroot = self.rustup / 'toolchains/1.99.0-x86_64-pc-windows-msvc'
        self.local = base / 'local'
        self.roots = {'repository': self.root, 'cargo-home': self.cargo,
                      'rust-sysroot': self.sysroot, 'tauri-nsis': self.local / 'tauri/NSIS'}
        for kind, names in guard.REQUIRED.items():
            for name in names:
                p = self.roots[kind] / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(('controlled input ' + kind + '/' + name).encode())
        (self.root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="1.99.0"\n', encoding='utf-8')
        (self.root / '.cargo/config.toml').write_text('[build]\nincremental=false\n', encoding='utf-8')
        self.config = self.root / 'apps/desktop/src-tauri/tauri.conf.json'
        self.config.write_text(json.dumps({'version': '0.1.22', 'bundle': {'targets': ['nsis']}}), encoding='utf-8')
        self.manifest = self.root / 'third-party/distribution-build-inputs.json'
        self.identity = {'schema': 1, 'upstreamPluginHistoryProven': False,
                         'rust': {'release': '1.99.0', 'commit': 'a' * 40, 'host': 'x86_64-pc-windows-msvc'},
                         'configurationSha256': guard.configuration_digest(self.root), 'files': {}}
        for kind, names in guard.REQUIRED.items():
            self.identity['files'][kind] = []
            for name in names:
                data = (self.roots[kind] / name).read_bytes()
                self.identity['files'][kind].append({'path': name, 'bytes': len(data), 'sha256': guard.sha(data)})
        self.save()
        self.env = {'CARGO_HOME': str(self.cargo), 'RUSTUP_HOME': str(self.rustup),
                    'LOCALAPPDATA': str(self.local), 'PATH': 'controlled-path'}
        self.probes = []

    def save(self):
        self.manifest.write_text(json.dumps(self.identity), encoding='utf-8')

    def which(self, command, path=None):
        self.assertEqual(path, 'controlled-path')
        return str(self.cargo / 'bin' / (command + '.exe'))

    def probe(self, exe, args, cwd):
        self.assertEqual(exe, self.sysroot / 'bin/rustc.exe')
        self.assertEqual(cwd, self.root)
        self.probes.append(args)
        if args == ['-vV']:
            return 'rustc 1.99.0\nrelease: 1.99.0\ncommit-hash: ' + 'a' * 40 + '\nhost: x86_64-pc-windows-msvc'
        self.assertEqual(args, ['--print', 'sysroot'])
        return str(self.sysroot)

    def check(self, **changes):
        return guard.check(self.root, **dict({'env': self.env, 'which': self.which, 'probe': self.probe}, **changes))

    def test_valid_inputs_and_only_absolute_installed_compiler_queries(self):
        result = self.check()
        self.assertEqual(result['files'], 24)
        self.assertFalse(result['upstreamPluginHistoryProven'])
        self.assertEqual(self.probes, [['-vV'], ['--print', 'sysroot']])

    def test_product_version_bump_preserves_identity_but_bundler_change_does_not(self):
        self.config.write_text(json.dumps({'version': '0.1.999', 'bundle': {'targets': ['nsis']}}), encoding='utf-8')
        self.check()
        self.config.write_text(json.dumps({'version': '0.1.999', 'bundle': {'targets': ['msi']}}), encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'Bundler configuration changed'):
            self.check()

    def test_toolchain_channel_drift_stops_before_probe(self):
        (self.root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="1.100.0"\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'Pinned Rust channel changed'):
            self.check()
        self.assertFalse(self.probes)

    def test_binary_notice_source_and_proxy_drift_stop_before_probe(self):
        cases = [('rust-sysroot', 'bin/rustc.exe'),
                 ('rust-sysroot', 'share/doc/rust/COPYRIGHT-library.html'),
                 ('tauri-nsis', 'Plugins/x86-unicode/additional/nsis_tauri_utils.dll'),
                 ('tauri-nsis', 'Bin/makensis.exe'),
                 ('tauri-nsis', 'Stubs/lzma_solid-x86-unicode'),
                 ('repository', 'third-party/distribution-inputs.json'),
                 ('repository', 'third-party/distribution-inputs.zip'),
                 ('repository', 'node_modules/@tauri-apps/cli-win32-x64-msvc/cli.win32-x64-msvc.node'),
                 ('cargo-home', 'bin/cargo.exe')]
        for kind, name in cases:
            with self.subTest(kind=kind, name=name):
                p = self.roots[kind] / name
                old = p.read_bytes()
                p.write_bytes(old + b' changed')
                try:
                    with self.assertRaisesRegex(ValueError, 'Build input changed'):
                        self.check()
                finally:
                    p.write_bytes(old)
        self.assertFalse(self.probes)

    def test_routing_overrides_are_rejected_without_disclosing_values(self):
        for name in ('CARGO', 'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTUP_TOOLCHAIN',
                     'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_TARGET_DIR',
                     'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER', 'TAURI_CONFIG',
                     'TAURI_BUNDLER_TOOLS_GITHUB_MIRROR', 'NSISDIR', 'XDG_CACHE_HOME'):
            with self.subTest(name=name):
                with self.assertRaises(ValueError) as error:
                    self.check(env=dict(self.env, **{name: 'sensitive-value-not-for-output'}))
                self.assertNotIn('sensitive-value-not-for-output', str(error.exception))
        self.assertFalse(self.probes)

    def test_path_proxy_route_cannot_be_replaced(self):
        with self.assertRaisesRegex(ValueError, 'Actual PATH route'):
            self.check(which=lambda *a, **k: str(self.root / 'unreviewed.exe'))
        self.assertFalse(self.probes)

    def test_missing_installed_toolchain_does_not_probe_or_install(self):
        old = self.env['RUSTUP_HOME']
        self.env['RUSTUP_HOME'] = str(Path(old) / 'missing')
        with self.assertRaisesRegex(ValueError, 'not installed'):
            self.check()
        self.assertFalse(self.probes)

    def test_nested_and_legacy_toolchain_files_are_rejected(self):
        for name in ('rust-toolchain', 'apps/desktop/rust-toolchain', 'apps/desktop/rust-toolchain.toml',
                     'apps/desktop/src-tauri/rust-toolchain.toml'):
            with self.subTest(name=name):
                p = self.root / name
                p.write_text('1.98.0', encoding='utf-8')
                try:
                    with self.assertRaisesRegex(ValueError, 'Additional Rust toolchain selection'):
                        self.check()
                finally:
                    p.unlink()
        self.assertFalse(self.probes)

    def test_external_and_nested_cargo_configuration_is_rejected(self):
        for p in (self.cargo / 'config.toml', self.root / '.cargo/config',
                  self.root / 'apps/desktop/.cargo/config.toml'):
            with self.subTest(path=p.name):
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text('[build]\nrustc="unreviewed"\n', encoding='utf-8')
                try:
                    with self.assertRaisesRegex(ValueError, 'Cargo configuration'):
                        self.check()
                finally:
                    p.unlink()
        self.assertFalse(self.probes)

    def test_rustup_directory_override_is_rejected(self):
        (self.rustup / 'settings.toml').write_text('[overrides]\n' + json.dumps(str(self.root)) + '="1.98.0"\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'Directory Rustup override'):
            self.check()
        self.assertFalse(self.probes)

    def test_probe_commit_or_sysroot_drift_is_rejected(self):
        for change in ('commit', 'sysroot'):
            def changed(exe, args, cwd):
                original = self.probe(exe, args, cwd)
                if args == ['-vV'] and change == 'commit':
                    return original.replace('a' * 40, 'b' * 40)
                if args == ['--print', 'sysroot'] and change == 'sysroot':
                    return str(self.root)
                return original
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'changed'):
                self.check(probe=changed)

    def test_identity_cannot_omit_required_inputs_or_claim_history_proven(self):
        original = copy.deepcopy(self.identity)
        for variation in ('missing', 'duplicate', 'path', 'schema', 'history'):
            self.identity = copy.deepcopy(original)
            records = self.identity['files']['tauri-nsis']
            if variation == 'missing':
                records.pop()
            elif variation == 'duplicate':
                records[-1] = records[0]
            elif variation == 'path':
                records[-1]['path'] = '../outside.dll'
            elif variation == 'schema':
                self.identity['schema'] = True
            else:
                self.identity['upstreamPluginHistoryProven'] = True
            self.save()
            with self.subTest(variation=variation), self.assertRaises(ValueError):
                self.check()
        self.assertFalse(self.probes)

    def test_identity_file_is_required_and_not_created_automatically(self):
        self.manifest.unlink()
        with self.assertRaises(FileNotFoundError):
            self.check()
        self.assertFalse(self.manifest.exists())
        self.assertFalse(self.probes)


class ProbeBounds(unittest.TestCase):
    class Process:
        def __init__(self, stdout=b'', stderr=b'', timeout=False):
            self.stdout, self.stderr = io.BytesIO(stdout), io.BytesIO(stderr)
            self.timeout = timeout
            self.killed = False

        def wait(self, timeout):
            if self.timeout and not self.killed:
                raise subprocess.TimeoutExpired('controlled probe', timeout)
            return 0

        def kill(self):
            self.killed = True

    def test_probe_caps_output_and_hides_stderr(self):
        for process in (self.Process(b'x' * (guard.PROBE_LIMIT + 1)), self.Process(stderr=b'private diagnostic')):
            with patch.object(guard.subprocess, 'Popen', return_value=process):
                with self.assertRaises(ValueError) as error:
                    guard.probe_installed_rustc(Path('installed.exe'), ['-vV'], Path('.'))
                self.assertNotIn('private diagnostic', str(error.exception))

    def test_deadline_applies_only_to_owned_readonly_probe(self):
        process = self.Process(timeout=True)
        with patch.object(guard.subprocess, 'Popen', return_value=process):
            with self.assertRaisesRegex(ValueError, 'deadline'):
                guard.probe_installed_rustc(Path('installed.exe'), ['-vV'], Path('.'))
        self.assertTrue(process.killed)


class EntryGate(unittest.TestCase):
    def test_optimized_python_is_rejected_before_resource_or_compiler_access(self):
        env = {k: v for k, v in os.environ.items()
               if not any(t in k.upper() for t in ('KEY', 'TOKEN', 'SECRET', 'CREDENTIAL', 'AUTH', 'PASSWORD', 'SIGN'))}
        result = subprocess.run([sys.executable, '-O', '-B', str(HERE / 'distribution-notices.py'), '--check'],
                                env=env, stdin=subprocess.DEVNULL, capture_output=True,
                                timeout=10, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'Distribution verification requires Python assertions enabled', result.stderr)
        self.assertNotIn(b'FileNotFoundError', result.stderr)
        self.assertEqual(result.stdout, b'')


if __name__ == '__main__':
    unittest.main(verbosity=2)
