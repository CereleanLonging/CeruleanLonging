"""Actual Windows sharing refusal and lost ACK in uninstrumented reader promotion."""
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT=Path(__file__).resolve().parent.parent
RUN=ROOT/'reports/reader-fault-125-cases'
NEW=ROOT/'reports/update-reader-123-gates/reader3-runtime/cerulean-runtime.exe'
OLD=ROOT/'reports/p6-windows-package-018-20261012/cerulean-runtime.exe'
calls=[]

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def write(path,value):
    with Path(path).open('x',encoding='utf-8') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
def snapshot(path):
    return {p.relative_to(path).as_posix():sha(p) for p in path.rglob('*') if p.is_file()}
def invoke(binary,args,payload=None,rejected=False):
    p=subprocess.Popen([str(binary),*map(str,args)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        out,err=p.communicate(None if payload is None else (json.dumps(payload,ensure_ascii=False)+'\n').encode(),timeout=25)
    except subprocess.TimeoutExpired:
        write(RUN/f'pending-{p.pid}.json',dict(pid=p.pid,binary=str(binary),args=list(map(str,args)),state='observation-pending-no-retry'))
        raise
    record=dict(pid=p.pid,binary=str(binary),args=list(map(str,args)),exit=p.returncode,stdoutEOF=True,stderrEOF=True,stdout=out.decode('utf-8'),stderr=err.decode('utf-8'))
    calls.append(record);write(RUN/f'call-{len(calls):03}.json',record)
    if rejected:
        assert p.returncode!=0,record
        return record
    assert p.returncode==0,record
    return json.loads(out)
def cli(data,args,rejected=False):
    result=invoke(NEW,['--workbench','--data-dir',data,*args],rejected=rejected)
    if rejected:return result
    assert result['ok'] is True,result
    return result['result']

kernel=ctypes.WinDLL('kernel32',use_last_error=True)
kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE]
kernel.CreateFileW.restype=wintypes.HANDLE
kernel.CloseHandle.argtypes=[wintypes.HANDLE];kernel.CloseHandle.restype=wintypes.BOOL
INVALID=ctypes.c_void_p(-1).value

def main():
    prior=sorted(RUN.glob('call-*.json'))
    assert len(prior)==8 and not (RUN/'resume-first-case.json').exists() and not (RUN/'result.json').exists(),'Resume only inspected first-case completion'
    calls.extend(json.loads(p.read_text(encoding='utf-8')) for p in prior)
    baseline=json.loads((ROOT/'reports/update-reader-123-gates/result.json').read_text(encoding='utf-8'))
    assert sha(NEW)==baseline['newRuntimeSha256']
    manifest=json.loads((OLD.parent/'manifest.json').read_text(encoding='utf-8-sig'))
    assert sha(OLD)==manifest['coreSha256']
    info=json.loads(calls[0]['stdout']);assert info['dataReaderGeneration']==3 and info['binaryUpdateSchema']==2
    cases=json.loads((RUN/'cases.json').read_text(encoding='utf-8'))
    first=cases[0];data=Path(first['data']);directory=data.parent;backup=Path(first['backup'])
    before=json.loads((directory/'before.json').read_text(encoding='utf-8'))
    completed=json.loads(calls[7]['stdout'])
    assert calls[7]['exit']==0 and completed['error'] is None and completed['result']['product']=='CeruleanLonging'
    assert not (data/'upgrade-hold.json').exists()
    assert all(sha(data/n)==h for n,h in before['data'].items() if n!='upgrade-hold.json')
    assert snapshot(backup)==before['backup']
    assert any(sha(p)==first['holdDigest'] for p in (data/'upgrade-holds').glob('released-*.json'))
    terminated=json.loads((directory/'terminated.json').read_text(encoding='utf-8'))
    assert terminated['ackBytes']==0 and terminated['exit']!=0
    assert sha(data/'events/00000000.reader3.json')==json.loads(calls[4]['stdout'])['result']['readerGuardDigest']
    assert calls[6]['exit']!=0 and 'missing field `body`' in calls[6]['stderr']
    write(RUN/'resume-first-case.json',dict(reason='Harness confused stdio response with workbench CLI ok envelope. Original successful snapshot and all completed first-case operations retained; no replay.',priorCalls=8,originalCaseCompleted=True,backupUnchanged=True,guardUnchanged=True))
    results=[dict(schema=1,realSharingRefusal=True,noWriteOnSharingRefusal=True,publishedDespiteMissingAck=True,ackBytes=0,
                  promotionReplayedAfterPublication=False,holdExplicitlyReleased=True,originalFilesPreserved=True,oldReaderRejected=True,currentReaderOpened=True,
                  childPid=terminated['pid'],exit=terminated['exit'],completedFromOriginalReceipts=True)]
    for case in cases[1:]:
        data=Path(case['data']);backup=Path(case['backup']);directory=data.parent
        args=['--from',backup,'--manifest-digest',case['manifestDigest'],'--hold-digest',case['holdDigest']]
        before=snapshot(data);backup_before=snapshot(backup)
        write(directory/'before.json',dict(data=before,backup=backup_before))
        reviewed=cli(data,['data-format','review-reader',*args])
        # Real handle denies another process write access, without ACL or trust changes.
        lock=kernel.CreateFileW(str(data/'writer.lock'),0x80000000,1,None,3,0,None)
        assert lock!=INVALID,ctypes.get_last_error()
        try:
            refused=cli(data,['data-format','promote-reader',*args,'--review-digest',reviewed['digest']],rejected=True)
            assert 'os error 32' in refused['stderr']+refused['stdout'],refused
            assert snapshot(data)==before and snapshot(backup)==backup_before
            write(directory/'share-refusal.json',dict(realWindowsShareReadHandle=True,error=refused['stderr'],dataBytesUnchanged=True,backupBytesUnchanged=True,holdRetained=True))
        finally:
            assert kernel.CloseHandle(lock)
        # A separate, explicit fault attempt follows only after zero-write refusal was proved.
        read_fd,write_fd=os.pipe();filler=b'P'*4096
        assert os.write(write_fd,filler)==len(filler)
        err=(directory/'promotion.stderr').open('xb')
        argv=['--workbench','--data-dir',str(data),'data-format','promote-reader',*map(str,args),'--review-digest',reviewed['digest']]
        p=subprocess.Popen([str(NEW),*argv],stdin=subprocess.DEVNULL,stdout=write_fd,stderr=err,creationflags=subprocess.CREATE_NO_WINDOW)
        os.close(write_fd)
        launch=dict(pid=p.pid,binary=str(NEW),sha256=sha(NEW),arguments=argv,instrumented=False,prefillBytes=4096)
        write(directory/'launch.json',launch)
        marker=data/'events/00000000.reader3.json';deadline=time.monotonic()+25
        while not marker.exists() and p.poll() is None and time.monotonic()<deadline:time.sleep(.01)
        if not marker.exists() or p.poll() is not None:
            write(directory/'pending.json',dict(**launch,observedExit=p.poll(),markerExists=marker.exists(),automaticRetry=False))
            raise RuntimeError('Publication and live original handle not confirmed; inspect original process')
        observed=cli(data,['data-format','status'])
        assert observed['dataSchema']==case['schema'] and observed['minimumReaderGeneration']==3
        guard=json.loads(marker.read_text(encoding='utf-8'))
        assert guard['sourceBackupDigest']==case['manifestDigest'] and (data/'upgrade-hold.json').is_file()
        assert p.poll() is None
        write(directory/'published-before-termination.json',dict(**launch,alive=True,status=observed,guard=guard,holdDigest=sha(data/'upgrade-hold.json')))
        # Controlled lost-ACK fault: terminate only this owned live Popen handle.
        p.kill()
        try:p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            write(directory/'pending-termination.json',dict(pid=p.pid,state='termination-requested-exit-unproven'));raise
        err.close()
        output=b''
        while True:
            chunk=os.read(read_fd,8192)
            if not chunk:break
            output+=chunk
        os.close(read_fd)
        assert output==filler and p.returncode!=0
        write(directory/'terminated.json',dict(pid=p.pid,exit=p.returncode,stdoutEOF=True,stderrWriterClosed=True,ackBytes=0,ownedHandleOnly=True))
        after=snapshot(data)
        assert set(after)-set(before)=={'events/00000000.reader3.json'}
        assert all(after[n]==h for n,h in before.items()) and snapshot(backup)==backup_before
        assert cli(data,['data-format','status'])==observed
        # Accept observed immutable publication. Do not invoke promote again.
        cli(data,['upgrade','release','--hold-digest',case['holdDigest']])
        rejection=invoke(OLD,['--stdio','--data-dir',data],payload={'id':'reader-fault125','protocolVersion':1,'command':{'type':'snapshot'}},rejected=True)
        assert 'missing field `body`' in rejection['stderr'],rejection
        current=invoke(NEW,['--stdio','--data-dir',data],payload={'id':'reader-fault125','protocolVersion':1,'command':{'type':'snapshot'}})
        assert current['error'] is None and current['result']['product']=='CeruleanLonging' and current['id']=='reader-fault125',current
        results.append(dict(schema=case['schema'],realSharingRefusal=True,noWriteOnSharingRefusal=True,publishedDespiteMissingAck=True,
                            ackBytes=0,promotionReplayedAfterPublication=False,holdExplicitlyReleased=True,originalFilesPreserved=True,
                            oldReaderRejected=True,currentReaderOpened=True,childPid=p.pid,exit=p.returncode))
    write(RUN/'result.json',dict(at=datetime.now(timezone.utc).isoformat(),status='passed',cases=results,calls=len(calls),buildInfo=info,
          runtimeSha256=sha(NEW),oldRuntimeSha256=sha(OLD),instrumentedRuntime=False,realModelCalls=0,userApplicationActions=0,
          limits=['Initial holds/backups/schema2 migration prepared by accepted fixture helper.',
                  'Real OS sharing denial occurs before pending-file creation. Mid-publication shared-handle/write-error and crash checkpoints not covered.',
                  'Lost ACK uses saturated stdout then termination after observed guard publication, not system power loss or disk-full.',
                  'Unpublished development binary, no release candidate or installed GUI claim.']))
    print(json.dumps(dict(status='passed',cases=results,calls=len(calls))))

if __name__=='__main__':main()
