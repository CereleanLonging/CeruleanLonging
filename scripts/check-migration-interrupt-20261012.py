"""Kill our instrumented migration child at four filesystem checkpoints.

The library test build adds deterministic pauses; it is not the shipped image.
All recovery/readability operations use frozen, hash-checked shipped runtimes.
No models, desktop applications, automatic migration retries, or OS restart.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile
import time
import uuid

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='migration-interrupt-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None

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
    for version, expected in [('015', '7266810b49283529c8b4952891138b97ad362d575d1bbb8a8754e6173c14ef32'),
                              ('014', 'a34f0555e843c032a2400b5c5a5c995bb731018c4e60a6c4072003ff13f6b3af')]:
        package = root / f'reports/p6-windows-package-{version}-20261012'
        assert digest(package / 'manifest.json') == expected
        manifest = json.loads((package / 'manifest.json').read_text(encoding='utf-8-sig'))
        exe = package / 'cerulean-runtime.exe'
        assert digest(exe) == next(x['sha256'] for x in manifest['artifacts'] if x['path'] == exe.name)
        package_hashes[str(exe)] = digest(exe)
        if version == '015':
            new = exe
        else:
            old = exe
    build = [json.loads(line) for line in load_log(root / 'reports/p6-migration-interrupt-build-20261012.jsonl').splitlines() if line.startswith('{')]
    assert build[-1] == {'reason': 'build-finished', 'success': True}
    artifacts = [x for x in build if x.get('reason') == 'compiler-artifact' and x['target']['name'] == 'cerulean_core' and x['profile']['test'] and x.get('executable')]
    assert len(artifacts) == 1
    test_exe = Path(artifacts[0]['executable'])
    test_hash = digest(test_exe)
    assert command(new, ['--build-info'])['version'] == '0.1.5'
    for phase in ['before-create', 'pending-created', 'pending-synced', 'marker-published']:
        case = run / phase; case.mkdir()
        data = case / 'data'; backup = case / 'backup'
        created = stdio(new, data, {'type': 'createPlan', 'title': '迁移进程中断', 'tasks': ['保留批准与原事件']})
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
        config_file = case / 'fault-config.json'; write(config_file, config)
        env = os.environ.copy(); env['CERULEAN_MIGRATION_FAULT_CONFIG'] = str(config_file)
        stdout = (case / 'child.stdout').open('wb'); stderr = (case / 'child.stderr').open('wb')
        p = subprocess.Popen([str(test_exe), '--exact', 'data_format::tests::migration_interrupt_child', '--ignored', '--nocapture'],
                             cwd=run, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                             creationflags=subprocess.CREATE_NO_WINDOW)
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
        p.kill()  # TerminateProcess on the Popen handle for our exact child.
        try:
            exit_code = p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pending.append({**launch, 'state': 'termination-requested-exit-not-yet-observed'})
            write(run / 'pending-processes.json', pending)
            raise RuntimeError('Original child exit must be observed before recovery')
        stdout.close(); stderr.close()
        assert exit_code != 0
        write(case / 'terminated.json', {**launch, 'exitCode': exit_code, 'hasExited': True,
                                        'liveReleaseRejection': live_rejection})
        after = fingerprint(data)
        assert all(after.get(name) == value for name, value in before.items())
        additional = sorted(set(after) - set(before))
        if phase == 'before-create':
            assert after == before
            assert cli(data, 'data-format', 'status')['result']['dataSchema'] == 1
        elif phase in ['pending-created', 'pending-synced']:
            assert len(additional) == 1 and additional[0].startswith('events/.format-migration-')
            assert additional[0].endswith('.pending') and not (data / 'events/00000000.json').exists()
            size = (data / additional[0]).stat().st_size
            assert (size == 0) == (phase == 'pending-created')
            cli(data, 'data-format', 'status', success=False)
            cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
            cli(data, 'data-format', 'migrate', *args, '--review-digest', migration['digest'], success=False)
            assert fingerprint(data) == after
        else:
            assert additional == ['events/00000000.json']
            current = cli(data, 'data-format', 'status')['result']
            assert current['dataSchema'] == 2 and current['sourceBackupDigest'] == manifest_digest
            cli(data, 'data-format', 'migrate', *args, '--review-digest', migration['digest'], success=False)
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
        item = {'phase': phase, 'pid': p.pid, 'exitCode': exit_code, 'hasExited': True,
                'testOnlyCheckpoint': True, 'originalFilesUnchanged': True, 'holdKept': True,
                'additionalFiles': additional, 'existingRestoreTargetRejected': True,
                'preservedEntireInterruptedDirectory': True, 'originalLocationRestoreExact': True,
                'legacyApprovedPlansRecovered': True, 'backupDigest': manifest_digest}
        cases.append(item); write(case / 'result.json', item)
    result = {'status': 'passed', 'run': str(run), 'cases': cases, 'passed': len(cases),
              'realChildTermination': True, 'instrumentedLibraryTestBuild': True,
              'frozenRuntimeRecovery': True, 'testExecutable': str(test_exe), 'testExecutableSha256': test_hash,
              'packageRuntimeHashes': package_hashes, 'shippedMigrationProcessKilled': False,
              'powerLossTested': False, 'diskFullTested': False, 'realModelCalls': 0,
              'userDesktopActions': 0, 'calls': calls}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'cases': cases, 'pending': pending, 'calls': calls}
finally:
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
