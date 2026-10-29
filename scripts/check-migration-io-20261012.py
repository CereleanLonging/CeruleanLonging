"""Real Windows byte-range write and share-delete publish failures.
Only checkpoint coordination uses a library test build; OS I/O is unchanged.
Recovery uses frozen 0.1.6 and legacy 0.1.4 runtimes. Not a disk-full test.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile
import time
import uuid
import ctypes
from ctypes import wintypes

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='migration-io-failure-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None
migration_child = None

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf8')

def load_log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

def command(exe, args, payload=None, success=True):
    number = len(calls) + 1
    stem = f'call-{number:03d}'
    p = subprocess.Popen([str(exe), *map(str, args)], cwd=run,
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        out, err = p.communicate(None if payload is None else (json.dumps(payload, ensure_ascii=False) + '\n').encode('utf8'), timeout=30)
    except subprocess.TimeoutExpired:
        pending.append({'pid': p.pid, 'executable': str(exe), 'args': list(map(str, args)),
                        'state': 'observation-timeout-do-not-restart'})
        write(run / 'pending-processes.json', pending)
        raise RuntimeError('Original process observation pending; no restart')
    (run / (stem + '.stdout')).write_bytes(out)
    (run / (stem + '.stderr')).write_bytes(err)
    calls.append({'pid': p.pid, 'executable': str(exe), 'args': list(map(str, args)),
                  'exit': p.returncode, 'expectedSuccess': success, 'evidence': stem})
    assert (p.returncode == 0) == success, (args, p.returncode, out, err)
    return json.loads(out) if success else {'exit': p.returncode, 'stdout': out.decode('utf8'), 'stderr': err.decode('utf8')}

def cli(data, *args, success=True):
    return command(new, ['--workbench', '--data-dir', data, *args], success=success)

def stdio(exe, data, operation, success=True):
    return command(exe, ['--stdio', '--data-dir', data],
                   {'id': 'migration-interrupt', 'protocolVersion': 1, 'command': operation}, success)

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): digest(p) for p in folder.rglob('*') if p.is_file()}

try:
    package_hashes = {}
    for version, expected in [('016', '2d6e34988565240c72978e1094d4e58f7a79cfa1296c159a3433df11d0e6bde3'),
                              ('014', 'a34f0555e843c032a2400b5c5a5c995bb731018c4e60a6c4072003ff13f6b3af')]:
        package = root / f'reports/p6-windows-package-{version}-20261012'
        assert digest(package / 'manifest.json') == expected
        manifest = json.loads((package / 'manifest.json').read_text(encoding='utf-8-sig'))
        exe = package / 'cerulean-runtime.exe'
        assert digest(exe) == next(x['sha256'] for x in manifest['artifacts'] if x['path'] == exe.name)
        package_hashes[str(exe)] = digest(exe)
        if version == '016':
            new = exe
        else:
            old = exe
    build = [json.loads(line) for line in load_log(root / 'reports/p6-migration-io-build-20261012.jsonl').splitlines() if line.startswith('{')]
    assert build[-1] == {'reason': 'build-finished', 'success': True}
    artifacts = [x for x in build if x.get('reason') == 'compiler-artifact' and x['target']['name'] == 'cerulean_core' and x['profile']['test'] and x.get('executable')]
    assert len(artifacts) == 1
    test_exe = Path(artifacts[0]['executable'])
    test_hash = digest(test_exe)
    assert command(new, ['--build-info'])['version'] == '0.1.6'
    for phase in ['pending-created', 'pending-synced']:
        case = run / phase; case.mkdir()
        data = case / 'data'; backup = case / 'backup'
        created = stdio(new, data, {'type': 'createPlan', 'title': '迁移存储故障', 'tasks': ['保留批准与原事件']})
        plan = created['result']['plans'][0]
        approved = stdio(new, data, {'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})
        original_plans = approved['result']['plans']; assert original_plans[0]['approved']
        active_core = data
        cli(data, 'service', 'start')
        review = cli(data, 'upgrade', 'review')['upgradeReview']; assert review['ready']
        hold = cli(data, 'upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']
        active_core = None
        manifest_digest = cli(data, 'upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
        before = fingerprint(data); assert before == fingerprint(backup / 'payload')
        args = ['--from', backup, '--manifest-digest', manifest_digest, '--hold-digest', hold]
        migration = cli(data, 'data-format', 'review', *args)['result']
        checkpoint = case / 'checkpoint.json'
        config = {'phase': phase, 'data': str(data), 'backup': str(backup), 'manifestDigest': manifest_digest,
                  'holdDigest': hold, 'reviewDigest': migration['digest'], 'checkpoint': str(checkpoint), 'token': uuid.uuid4().hex}
        config['resume'] = str(case / 'resume-token'); config['outcome'] = str(case / 'io-error.txt')
        config_file = case / 'fault-config.json'; write(config_file, config)
        env = os.environ.copy(); env['CERULEAN_MIGRATION_FAULT_CONFIG'] = str(config_file)
        stdout = (case / 'child.stdout').open('wb'); stderr = (case / 'child.stderr').open('wb')
        p = subprocess.Popen([str(test_exe), '--exact', 'data_format::tests::migration_io_failure_child', '--ignored', '--nocapture'],
                             cwd=run, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                             creationflags=subprocess.CREATE_NO_WINDOW)
        migration_child = p
        launch = {'pid': p.pid, 'executable': str(test_exe), 'sha256': test_hash, 'phase': phase,
                  'testOnlyCheckpoint': True, 'configSha256': digest(config_file)}
        write(case / 'launch.json', launch)
        deadline = time.monotonic() + 30
        receipt = None
        while time.monotonic() < deadline:
            if checkpoint.exists():
                try:
                    receipt = json.loads(checkpoint.read_text(encoding='utf8'))
                except json.JSONDecodeError:
                    pass
                if receipt:
                    break
            if p.poll() is not None:
                break
            time.sleep(0.02)
        if not receipt:
            if p.poll() is None:
                pending.append({**launch, 'state': 'checkpoint-not-observed-original-process-preserved'})
                write(run / 'pending-processes.json', pending)
            raise RuntimeError(f'{phase}: original checkpoint not observed; no retry')
        assert receipt == {'phase': phase, 'pid': p.pid, 'token': config['token'], 'testOnlyCheckpoint': True}
        assert p.poll() is None, 'Checkpoint child unexpectedly exited'
        # Verify the real migration child still owns the writer; no release or
        # competing migration can pass while this exact process is suspended.
        live_rejection = cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
        assert (data / 'upgrade-hold.json').exists()
        assert p.poll() is None
        temporary = list((data / 'events').glob('.format-migration-*.pending'))
        assert len(temporary) == 1
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
        kernel.CreateFileW.restype = wintypes.HANDLE
        kernel.LockFile.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD, wintypes.DWORD]
        kernel.LockFile.restype = wintypes.BOOL
        kernel.UnlockFile.argtypes = kernel.LockFile.argtypes
        kernel.UnlockFile.restype = wintypes.BOOL
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel.CloseHandle.restype = wintypes.BOOL
        handle = kernel.CreateFileW(str(temporary[0]), 0xC0000000, 3, None, 3, 0x80, None)
        assert handle != ctypes.c_void_p(-1).value, ctypes.get_last_error()
        byte_locked = False
        try:
            if phase == 'pending-created':
                assert kernel.LockFile(handle, 0, 0, 1048576, 0), ctypes.get_last_error()
                byte_locked = True
            # The marker is not edited or truncated by the fixture. The real
            # migration writer resumes while another process owns the OS lock.
            Path(config['resume']).write_text(config['token'], encoding='utf8')
            try:
                exit_code = p.wait(timeout=30)
            except subprocess.TimeoutExpired:
                pending.append({**launch, 'state': 'resumed-original-process-awaiting-exit'})
                write(run / 'pending-processes.json', pending)
                raise RuntimeError('Original migration process pending; no retry')
        finally:
            if byte_locked:
                assert kernel.UnlockFile(handle, 0, 0, 1048576, 0), ctypes.get_last_error()
            assert kernel.CloseHandle(handle), ctypes.get_last_error()
        migration_child = None
        stdout.close(); stderr.close()
        assert exit_code == 0, 'Test child must observe an actual I/O failure'
        error = Path(config['outcome']).read_text(encoding='utf8')
        expected_error = 33 if phase == 'pending-created' else 32
        assert f'(os error {expected_error})' in error, error
        write(case / 'io-failure.json', {**launch, 'testExitCode': exit_code,
                'migrationReturnedError': error, 'windowsError': expected_error,
                'hasExited': True, 'processKilled': False,
                'lockType': 'byte-range' if byte_locked else 'deny-share-delete',
                'liveReleaseRejection': live_rejection})
        after = fingerprint(data)
        assert all(after.get(name) == value for name, value in before.items())
        additional = sorted(set(after) - set(before))
        assert len(additional) == 1 and additional[0].startswith('events/.format-migration-')
        assert additional[0].endswith('.pending') and not (data / 'events/00000000.json').exists()
        size = (data / additional[0]).stat().st_size
        assert (size == 0) == (phase == 'pending-created')
        cli(data, 'data-format', 'status', success=False)
        cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
        cli(data, 'data-format', 'migrate', *args, '--review-digest', migration['digest'], success=False)
        stdio(new, data, {'type': 'snapshot'}, success=False)
        assert fingerprint(data) == after
        # Existing recovery target never gets overwritten, even after a failure.
        cli(data, 'backup', 'restore', '--from', backup, '--manifest-digest', manifest_digest, success=False)
        assert fingerprint(data) == after and (data / 'upgrade-hold.json').exists()
        preserved = case / 'preserved-interrupted-data'
        assert data.resolve().parent == case.resolve() and preserved.resolve().parent == case.resolve()
        assert case.resolve().parent == run.resolve() and not preserved.exists()
        os.rename(data, preserved)
        restored = cli(data, 'backup', 'restore', '--from', backup, '--manifest-digest', manifest_digest)['result']
        assert restored['manifestDigest'] == manifest_digest and fingerprint(data) == before
        assert (data / 'upgrade-hold.json').exists()
        assert cli(data, 'data-format', 'status')['result']['dataSchema'] == 1
        cli(data, 'upgrade', 'release', '--hold-digest', hold)
        assert stdio(old, data, {'type': 'snapshot'})['result']['plans'] == original_plans
        item = {'phase': phase, 'pid': p.pid, 'testExitCode': exit_code, 'hasExited': True, 'windowsError': expected_error, 'migrationReturnedError': error,
                'testOnlyCheckpoint': True, 'originalFilesUnchanged': True, 'holdKept': True,
                'additionalFiles': additional, 'existingRestoreTargetRejected': True,
                'preservedEntireInterruptedDirectory': True, 'originalLocationRestoreExact': True, 'automaticPreservationImplemented': False,
                'legacyApprovedPlansRecovered': True, 'backupDigest': manifest_digest}
        cases.append(item); write(case / 'result.json', item)
    result = {'status': 'passed', 'run': str(run), 'cases': cases, 'passed': len(cases),
              'realWindowsIoFailure': True, 'realChildTermination': False, 'instrumentedLibraryTestBuild': True,
              'frozenRuntimeRecovery': True, 'testExecutable': str(test_exe), 'testExecutableSha256': test_hash,
              'packageRuntimeHashes': package_hashes, 'shippedMigrationProcessKilled': False,
              'powerLossTested': False, 'diskFullTested': False, 'realModelCalls': 0,
              'userDesktopActions': 0, 'calls': calls}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'cases': cases, 'pending': pending, 'calls': calls}
finally:
    if migration_child is not None and migration_child.poll() is None:
        if not any(x['pid'] == migration_child.pid for x in pending):
            pending.append({'pid': migration_child.pid, 'state': 'original-test-child-still-live'})
        write(run / 'pending-processes.json', pending)
    cleanup = 'passed'
    if active_core is not None and not pending:
        try:
            cli(active_core, 'service', 'stop')
        except Exception as exc:
            cleanup = str(exc)
    if pending:
        cleanup = 'original-process-preserved'
    result['cleanup'] = cleanup
    write(run / 'result.json', result)
    print(json.dumps(result, ensure_ascii=False))
if result['status'] != 'passed' or cleanup != 'passed':
    raise SystemExit(1)
