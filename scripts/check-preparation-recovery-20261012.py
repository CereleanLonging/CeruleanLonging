"""Owned test-build preparation interruption; normal core diagnoses/abandons.
Three real child exits at test-only checkpoints, plus one explicit corrupt fixture.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile
import time
import uuid
import traceback

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='preparation-interrupt-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None
worker = None

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
                   {'id': 'recovery-transaction', 'protocolVersion': 1, 'command': operation}, success)

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): digest(p) for p in folder.rglob('*') if p.is_file()}



try:
    new = root / 'target/debug/cerulean-core.exe'
    build = [json.loads(line) for line in load_log(root / 'reports/p6-preparation-recovery-build-tests-20261012.jsonl').splitlines() if line.startswith('{')]
    assert build[-1] == {'reason': 'build-finished', 'success': True}
    artifacts = [x for x in build if x.get('reason') == 'compiler-artifact' and x['target']['name'] == 'cerulean_core' and x['profile']['test'] and x.get('executable')]
    assert len(artifacts) == 1
    test_exe = Path(artifacts[0]['executable']); test_hash = digest(test_exe)
    for phase in ['metadata-file-created', 'guard-published', 'payload-copied', 'malformed-prepared-fixture']:
        case = run / phase; case.mkdir(); data = case / 'data'; backup = case / 'backup'
        plan = stdio(new, data, {'type': 'createPlan', 'title': '准备中断保留证据', 'tasks': ['原批准不可改写']})['result']['plans'][0]
        stdio(new, data, {'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})
        active_core = data; cli(data, 'service', 'start')
        review = cli(data, 'upgrade', 'review')['upgradeReview']; assert review['ready']
        hold = cli(data, 'upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']; active_core = None
        manifest = cli(data, 'upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
        (data / 'events/.format-migration-prep-fixture.pending').write_bytes(b'original fault evidence')
        before = fingerprint(data)
        args = ['--from', backup, '--manifest-digest', manifest, '--hold-digest', hold]
        reviewed = cli(data, 'recovery', 'review', *args)['result']; tx = Path(reviewed['transaction'])
        interrupted = phase != 'malformed-prepared-fixture'; pid = None; exit_code = None
        if interrupted:
            checkpoint = case / 'checkpoint.json'
            config = {'phase': phase, 'data': str(data), 'backup': str(backup), 'manifestDigest': manifest, 'holdDigest': hold, 'reviewDigest': reviewed['digest'], 'checkpoint': str(checkpoint), 'token': uuid.uuid4().hex}
            config_file = case / 'fault-config.json'; write(config_file, config)
            env = os.environ.copy(); env['CERULEAN_PREPARATION_FAULT_CONFIG'] = str(config_file)
            stdout = (case / 'child.stdout').open('wb'); stderr = (case / 'child.stderr').open('wb')
            worker = subprocess.Popen([str(test_exe), '--exact', 'backup_recovery::tests::preparation_interrupt_child', '--ignored', '--nocapture'], cwd=run, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, creationflags=subprocess.CREATE_NO_WINDOW)
            launch = {'pid': worker.pid, 'phase': phase, 'testExecutableSha256': test_hash, 'instrumentedTestBuild': True}
            write(case / 'launch.json', launch)
            deadline = time.monotonic()+30; receipt = None
            while time.monotonic() < deadline and worker.poll() is None:
                if checkpoint.exists():
                    try: receipt = json.loads(checkpoint.read_text(encoding='utf8'))
                    except json.JSONDecodeError: pass
                    if receipt: break
                time.sleep(0.02)
            assert receipt == {'phase': phase, 'pid': worker.pid, 'token': config['token'], 'testOnlyCheckpoint': True}, 'Original preparation checkpoint unproven; no retry'
            assert worker.poll() is None
            cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
            worker.kill(); exit_code = worker.wait(timeout=10); pid = worker.pid; worker = None
            stdout.close(); stderr.close(); assert exit_code != 0
            write(case / 'terminated.json', {**launch, 'hasExited': True, 'exitCode': exit_code})
        else:
            cli(data, 'recovery', 'prepare', *args, '--review-digest', reviewed['digest'])
            (tx / 'prepared.json').write_bytes(b'{incomplete-prepared-record')
        diagnosis = cli(data, 'recovery', 'diagnose')['result']
        assert fingerprint(data) == before and not diagnosis['automaticActions']
        if phase == 'metadata-file-created':
            assert not diagnosis['guardPresent'] and not tx.exists()
            assert len(diagnosis['unpublishedMetadata']) == 1
            leftover = Path(diagnosis['unpublishedMetadata'][0]); assert (leftover / 'plan.json').stat().st_size == 0
            assert cli(data, 'recovery', 'review', *args)['result']['digest'] == reviewed['digest']
            assert leftover.exists() and fingerprint(data) == before
            disposition = 'unpublished metadata retained; original hold unchanged; new review available'
        else:
            assert diagnosis['guardPresent'] and diagnosis['canAbandonPreparation']
            assert diagnosis['preparedRecord'] == ('invalid' if not interrupted else 'missing')
            assert diagnosis['nextAction'] == 'review-and-abandon-preparation'
            before_abandon = fingerprint(tx)
            cli(data, 'recovery', 'apply', '--review-digest', reviewed['digest'], success=False)
            assert fingerprint(tx) == before_abandon
            archived = Path(cli(data, 'recovery', 'abandon', '--review-digest', reviewed['digest'])['result'])
            after = fingerprint(archived)
            assert all(after.get(name) == value for name, value in before_abandon.items())
            assert set(after)-set(before_abandon) == {'finalization.json'}
            assert json.loads((archived / 'finalization.json').read_text(encoding='utf8'))['schema'] == 2
            assert fingerprint(data) == before and not tx.exists()
            observed = cli(data, 'recovery', 'inspect', '--review-digest', reviewed['digest'])['result']
            assert observed['archives'][0]['verified'] and observed['archives'][0]['disposition'] == 'abandoned'
            disposition = 'explicitly abandoned; all original evidence bytes preserved'
        item = {'phase': phase, 'pid': pid, 'exitCode': exit_code, 'realChildTermination': interrupted, 'testOnlyCheckpoint': interrupted, 'originalFilesUnchanged': True, 'holdKept': True, 'disposition': disposition, 'diagnosis': diagnosis}
        cases.append(item); write(case / 'result.json', item)
    previous = json.loads((root / 'reports/p6-recovery-inspect-20261012.json').read_text(encoding='utf8'))
    compatibility = []
    for case in previous['native']['cases']:
        archived = Path(case['archive']); original = fingerprint(archived)
        sealed = json.loads((archived / 'finalization.json').read_text(encoding='utf8')); assert sealed['schema'] == 1
        plan = json.loads((archived / 'plan.json').read_text(encoding='utf8'))
        observed = cli(Path(plan['sourceRoot']), 'recovery', 'inspect', '--review-digest', plan['digest'])['result']
        matching = [x for x in observed['archives'] if Path(x['archive']).samefile(archived)]
        assert len(matching) == 1 and matching[0]['verified'] and fingerprint(archived) == original
        compatibility.append({'archive': str(archived), 'schema': 1, 'verifiedReadOnly': True})
    result = {'status': 'passed', 'run': str(run), 'cases': cases, 'compatibility': compatibility, 'calls': calls, 'developmentCoreSha256': digest(new), 'testExecutableSha256': test_hash, 'realChildTerminations': 3, 'testOnlyCheckpoints': True, 'malformedRecordCaseIsExplicitFixture': True, 'shippedReleaseAccepted': False, 'diskFullTested': False, 'powerLossTested': False, 'modelCalls': 0, 'userDesktopActions': 0}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'traceback': traceback.format_exc(), 'cases': cases, 'calls': calls}
finally:
    if worker is not None and worker.poll() is None:
        pending.append({'pid': worker.pid, 'state': 'original-preparation-child-still-live-do-not-restart'})
        write(run / 'pending-processes.json', pending)
    cleanup = 'passed'
    if active_core is not None and not pending:
        try: cli(active_core, 'service', 'stop')
        except Exception as exc: cleanup = str(exc)
    if pending: cleanup = 'original-process-preserved'
    result['cleanup'] = cleanup; result['pending'] = pending
    write(run / 'result.json', result); print(json.dumps(result, ensure_ascii=False))
if result['status'] != 'passed' or cleanup != 'passed': raise SystemExit(1)
