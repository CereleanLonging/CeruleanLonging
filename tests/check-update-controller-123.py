"""Owned controller fixture: real CLI reservations/reviews, synthetic terminal journal."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent.parent
RUN = ROOT / 'reports/update-reader-123-controller-cases'
NEW = ROOT / 'target/debug/cerulean-core.exe'
OLD = ROOT / 'reports/p6-windows-package-018-20261012/cerulean-runtime.exe'
PS = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
calls = []


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def write(p, v):
    with Path(p).open('x', encoding='utf-8') as f:
        json.dump(v, f, ensure_ascii=False, indent=2)
    return sha(p)


def snapshot(directory):
    return {p.relative_to(directory).as_posix(): sha(p) for p in sorted(directory.rglob('*')) if p.is_file()}


def call(binary, args, reject=''):
    p = subprocess.Popen([str(binary), *map(str, args)], stdin=subprocess.DEVNULL,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        out, err = p.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        write(RUN / f'call-{len(calls)+1:03}-unresolved.json', dict(pid=p.pid, binary=str(binary), arguments=list(map(str,args)), unresolved=True))
        raise RuntimeError('Unresolved child; inspect original process/state, no retry')
    r = dict(pid=p.pid, binary=str(binary), arguments=list(map(str,args)), exitCode=p.returncode,
             stdoutEOF=True, stderrEOF=True, stdout=out.decode('utf-8-sig',errors='replace'), stderr=err.decode('utf-8-sig',errors='replace'))
    calls.append(r)
    write(RUN / f'call-{len(calls):03}.json', r)
    if reject:
        assert p.returncode != 0 and reject in r['stdout']+r['stderr'], r
        return r
    assert p.returncode == 0, r
    return json.loads(r['stdout'])


def main():
    prior = sorted(RUN.glob('call-*.json'))
    assert len(prior) == 3 and not (RUN/'resume-original.json').exists(), 'Resume only inspected first-run state'
    calls.extend(json.loads(p.read_text(encoding='utf-8')) for p in prior)
    fixture = json.loads((RUN/'fixture.json').read_text(encoding='utf-8'))
    data = Path(fixture['data'].removeprefix('\\\\?\\'))
    tx = Path(fixture['transaction'])
    install = Path(fixture['install'])
    intent_path = tx/'intent.json'
    intent = json.loads(intent_path.read_text(encoding='utf-8'))
    intent_digest = sha(intent_path)
    reviewed_fixture = json.loads((RUN/'reviewed-fixture.json').read_text(encoding='utf-8'))
    assert intent_digest == reviewed_fixture['intentDigest']
    assert {p.name for p in tx.iterdir()} == {'intent.json','transaction.lock'}
    assert sha(install/'program.exe') == hashlib.sha256(b'original fixture program').hexdigest()
    def cli(args, binary=NEW, reject=''):
        result = call(binary, ['--workbench','--data-dir',data,*args], reject)
        if reject:
            return result
        assert result['ok'] is True, result
        return result['result']
    reserved = json.loads(calls[0]['stdout'])['result']
    assert calls[0]['exitCode'] == 0 and calls[1]['exitCode'] == 0
    assert calls[2]['exitCode'] == 3 and json.loads(calls[2]['stderr'])['error']['message'] == 'Maintenance reservation identity differs'
    guard = reserved['guardDigest']
    assert reserved['reservation']['schema'] == 2
    assert cli(['maintenance','check','--guard-digest',guard])['state'] == 'reserved'
    before = snapshot(tx.parent)
    write(RUN/'resume-original.json',dict(reason='First harness expected lowercase error text; old core correctly rejected with capital M. No reservation repeated.',before=before,priorCalls=3,originalIntentUnchanged=True,currentDataVerifiedByCore=True))
    # Old binary cannot consume or release this schema-2 reservation.
    for command in ('abandon',):
        r = call(OLD,['--workbench','--data-dir',data,'maintenance',command,'--guard-digest',guard], reject='Maintenance reservation identity differs')
        assert before == snapshot(tx.parent), 'Old core changed fixture'
    backup = cli(['upgrade','backup','--hold-digest',fixture['holdDigest'],'--to',tx/'data-backup'])
    plan = dict(intent,backupDigest=backup['manifestDigest'],newEntries=intent['oldEntries'])
    digest = write(tx/'plan.json',plan)
    previous = write(tx/'event-001.json',dict(schema=2,sequence=1,previousDigest=digest,kind='prepared'))
    def controller(script,action='Review',rejection=''):
        return call(PS,['-NoProfile','-NonInteractive','-ExecutionPolicy','RemoteSigned','-File',script,
                        '-Action',action,'-TransactionDirectory',tx,'-PlanDigest',digest],rejection)
    before = snapshot(tx.parent)
    controller(ROOT/'reports/update-reader-123-original-controller/manage-windows-update.ps1',rejection='Transaction identity')
    assert before == snapshot(tx.parent)
    reviewed = controller(ROOT/'scripts/manage-windows-update.ps1')
    assert reviewed['state'] == 'prepared' and reviewed['maintenance']['reservation']['schema'] == 2
    assert before == snapshot(tx.parent), 'Review mutated fixture'
    # Synthetic controlled terminal journal, clearly not a real program Rollback.
    for index,kind in ((2,'rollback-intent'),(3,'rolled-back')):
        previous = write(tx/f'event-{index:03}.json',dict(schema=2,sequence=index,previousDigest=previous,kind=kind))
    released = cli(['maintenance','release','--guard-digest',guard,'--plan-digest',digest])
    assert released['state'] == 'released'
    assert cli(['maintenance','status','--guard-digest',guard])['state'] == 'released'
    assert (data/'upgrade-hold.json').is_file()
    result = dict(at=datetime.now(timezone.utc).isoformat(),status='passed',callCount=len(calls),
                  coreSha256=sha(NEW),oldCoreSha256=sha(OLD),intentDigest=intent_digest,planDigest=digest,
                  guardDigest=guard,oldCoreAndControllerRejectedWithoutWrites=True,newControllerReview=reviewed,
                  released=released,realModelCalls=0,userDesktopActions=0,installed=False,
                  limits=['Owned synthetic initial hold and program inventory; actual reservation/check/review/release CLI.',
                          'Terminal rollback journal constructed by fixture, no real installed Prepare/Apply/Rollback.',
                          'Original data hold intentionally remains in isolated evidence directory; no user data or installation changed.'])
    write(RUN/'result.json',result)
    print(json.dumps(dict(status='passed',calls=len(calls))))


if __name__ == '__main__':
    main()
