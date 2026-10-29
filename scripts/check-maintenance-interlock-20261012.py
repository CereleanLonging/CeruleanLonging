"""Actual CLI and PowerShell lock interoperability, synthetic program journals.
No real installation/registry/update-controller integration is claimed here.
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
run = Path(tempfile.mkdtemp(prefix='maintenance-interlock-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None
owner = None

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
    assert command(new, ['--build-info'])['maintenanceInterlockSchema'] == 1
    case = run / 'fixture'; case.mkdir(); data = case / 'data'; backup = case / 'recovery-backup'; install = case / 'installed'; tx = case / 'transaction'
    install.mkdir(); tx.mkdir(); (install / 'fixture-program.exe').write_bytes(b'not executable; program inventory fixture')
    (tx / 'transaction.lock').write_bytes(b'')
    stdio(new, data, {'type': 'createPlan', 'title': '维护互斥夹具', 'tasks': ['保持原数据']})
    active_core = data; cli(data, 'service', 'start')
    review = cli(data, 'upgrade', 'review')['upgradeReview']; assert review['ready']
    hold = cli(data, 'upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']; active_core = None
    manifest = cli(data, 'upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
    baseline = fingerprint(data)
    recovery_args = ['--from', backup, '--manifest-digest', manifest, '--hold-digest', hold]
    recovery_review = cli(data, 'recovery', 'review', *recovery_args)['result']
    original_hold = json.loads((data / 'upgrade-hold.json').read_text(encoding='utf8'))
    program = install / 'fixture-program.exe'
    entries = [{'path': program.name, 'directory': False, 'bytes': program.stat().st_size, 'sha256': digest(program)}]
    intent = {'schema':1,'ownerSid':original_hold['ownerSid'],'transaction':str(tx),'data':str(data),'install':str(install),'holdDigest':hold,'sourceVersion':'0.1.6','targetVersion':'0.1.7','sourceManifest':'a'*64,'targetManifest':'b'*64,'backupDigest':'','oldEntries':entries,'newEntries':[]}
    write(tx / 'intent.json', intent); intent_digest = digest(tx / 'intent.json')
    actor = run / 'lock-owner.ps1'
    actor.write_text("""param([string]$LockPath,[string]$Ready,[string]$Release)
$ErrorActionPreference='Stop'
$lock=[IO.File]::Open($LockPath,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try{
 $readyTemp=$Ready+'.pending'
 [IO.File]::WriteAllText($readyTemp,('{"pid":'+$PID+'}'))
 [IO.File]::Move($readyTemp,$Ready)
 while(-not (Test-Path -LiteralPath $Release)){Start-Sleep -Milliseconds 100}
}finally{$lock.Dispose()}
""", encoding='utf-8-sig')
    ready = run / 'owner-ready.json'; signal = run / 'release-owner'
    out = (run / 'owner.stdout').open('wb'); err = (run / 'owner.stderr').open('wb')
    owner = subprocess.Popen(['powershell.exe','-NoProfile','-NonInteractive','-ExecutionPolicy','RemoteSigned','-WindowStyle','Hidden','-File',str(actor),'-LockPath',str(tx/'transaction.lock'),'-Ready',str(ready),'-Release',str(signal)],stdin=subprocess.DEVNULL,stdout=out,stderr=err,creationflags=subprocess.CREATE_NO_WINDOW,cwd=run)
    deadline = time.monotonic()+30
    while not ready.exists() and owner.poll() is None and time.monotonic()<deadline: time.sleep(0.02)
    assert ready.exists() and owner.poll() is None
    owner_receipt = json.loads(ready.read_text(encoding='utf8')); assert owner_receipt['pid'] == owner.pid
    reserve_args = ['--transaction',tx,'--intent-digest',intent_digest,'--hold-digest',hold]
    reserved = cli(data,'maintenance','reserve',*reserve_args)['result']; guard = reserved['guardDigest']
    assert reserved['state']=='reserved' and owner.poll() is None
    # The producer's live FileShare.None transaction lock prevents a second
    # process from finalizing, even though it can acquire the reservation.
    cli(data,'maintenance','abandon','--guard-digest',guard,success=False)
    cli(data,'recovery','prepare',*recovery_args,'--review-digest',recovery_review['digest'],success=False)
    cli(data,'upgrade','release','--hold-digest',hold,success=False)
    cli(data,'service','start',success=False)
    assert fingerprint(data)==baseline and owner.poll() is None
    signal.write_text('release',encoding='utf8')
    exit_code = owner.wait(timeout=10); owner_pid = owner.pid; owner = None; out.close(); err.close(); assert exit_code==0
    assert cli(data,'maintenance','status','--guard-digest',guard)['result']['state']=='reserved'
    assert cli(data,'maintenance','check','--guard-digest',guard)['result']['state']=='reserved'
    assert cli(data,'maintenance','abandon','--guard-digest',guard)['result']['state']=='released'
    assert cli(data,'maintenance','status','--guard-digest',guard)['result']['state']=='released'
    assert fingerprint(data)==baseline
    cli(data,'recovery','prepare',*recovery_args,'--review-digest',recovery_review['digest'])
    cli(data,'maintenance','reserve',*reserve_args,success=False)
    assert cli(data,'maintenance','status')['result']['state']=='unreserved'
    cli(data,'recovery','abandon','--review-digest',recovery_review['digest'])
    assert fingerprint(data)==baseline
    # New durable reservation; terminal journal is an explicitly synthetic
    # rollback fixture. This tests independent validation, not an installer.
    reserved2 = cli(data,'maintenance','reserve',*reserve_args)['result']; guard2 = reserved2['guardDigest']; assert guard2 != guard
    manifest2 = cli(data,'upgrade','backup','--hold-digest',hold,'--to',tx/'data-backup')['result']['manifestDigest']
    plan = dict(intent); plan['backupDigest']=manifest2; plan['newEntries']=entries
    write(tx/'plan.json',plan); plan_digest=digest(tx/'plan.json'); previous=plan_digest
    for i,kind in enumerate(['prepared','rollback-intent','rolled-back'],1):
        event=tx/f'event-{i:03d}.json';write(event,{'schema':1,'sequence':i,'previousDigest':previous,'kind':kind});previous=digest(event)
    cli(data,'maintenance','abandon','--guard-digest',guard2,success=False)
    assert cli(data,'maintenance','release','--guard-digest',guard2,'--plan-digest',plan_digest)['result']['state']=='released'
    assert fingerprint(data)==baseline and digest(program)==entries[0]['sha256']
    assert cli(data,'maintenance','status','--guard-digest',guard2)['result']['state']=='released'
    result={'status':'passed','run':str(run),'calls':calls,'developmentCoreSha256':digest(new),'powershellOwnerPid':owner_pid,'powershellOwnerExit':exit_code,'reservationSurvivedControllerExit':True,'liveExclusiveTransactionLockRejectedRelease':True,'bothAcquisitionOrdersRejectedOverlap':True,'originalDataBytesUnchanged':True,'originalProgramBytesUnchanged':True,'originalHoldPreserved':True,'releasedGuardReadbackVerified':True,'terminalRollbackJournalIsSynthetic':True,'actualInstallerOrUpdateControllerTested':False,'realModelCalls':0,'userDesktopActions':0,'shippedReleaseAccepted':False}
except Exception as exc:
    result={'status':'failed','run':str(run),'error':str(exc),'traceback':traceback.format_exc(),'calls':calls}
finally:
    if owner is not None and owner.poll() is None:
        pending.append({'pid':owner.pid,'state':'original-lock-owner-still-live-do-not-restart'})
        write(run/'pending-processes.json',pending)
    cleanup='passed'
    if active_core is not None and not pending:
        try: cli(active_core,'service','stop')
        except Exception as exc: cleanup=str(exc)
    if pending: cleanup='original-process-preserved'
    result['cleanup']=cleanup;result['pending']=pending
    write(run/'result.json',result);print(json.dumps(result,ensure_ascii=False))
if result['status']!='passed' or cleanup!='passed':raise SystemExit(1)
