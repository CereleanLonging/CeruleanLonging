import ctypes,hashlib,json,subprocess
from datetime import datetime
from pathlib import Path
R=Path(__file__).resolve().parent.parent; run=R/'reports'/('candidate-overwrite-share-20261013T'+datetime.now().strftime('%H%M%S%f'));run.mkdir(parents=True)
S=R/'reports/p6-windows-package-017-20261012';C=R/'reports/p6-windows-package-018-20261012';I=run/'installed app';D=run/'data';P=run/'project';T=run/'transaction';U=R/'scripts/manage-windows-update.ps1';calls=[];counts={}
def h(p):
 x=hashlib.sha256();f=open('\\\\?\\'+str(Path(p).resolve()),'rb')
 for b in iter(lambda:f.read(1048576),b''):x.update(b)
 f.close();return x.hexdigest()
def inv(p):return sorted([{'path':str(x.relative_to(p)).replace('\\','/'),'bytes':x.stat().st_size,'sha256':h(x)} for x in Path(p).rglob('*') if x.is_file()],key=lambda z:z['path'])
def save(n,v): (run/n).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
def call(a,e=0):
 q=subprocess.Popen(a,stdout=subprocess.PIPE,stderr=subprocess.PIPE);o,r=q.communicate(timeout=240);z={'args':list(map(str,a)),'pid':q.pid,'exit':q.returncode,'stdoutEOF':True,'stderrEOF':True,'stdout':o.decode('utf-8-sig','replace'),'stderr':r.decode('utf-8-sig','replace')};calls.append(z);save('call-%03d.json'%len(calls),z)
 if q.returncode!=e:raise RuntimeError('exit %s expected %s'%(q.returncode,e))
 return json.loads(o.decode('utf-8-sig')) if o.strip() and a[0].lower().endswith('.exe') else z
def core(*a,e=0):return call([str(I/'cerulean-runtime.exe'),'--workbench','--data-dir',str(D),*map(str,a)],e)
def ps(*a,e=0):return call(['powershell.exe','-NoProfile','-ExecutionPolicy','RemoteSigned','-File',str(U),*map(str,a)],e)
r={'status':'failed','run':str(run),'scenario':'candidate-executable-overwrite-real-share-rejection','scope':'direct CLI/PowerShell isolated installation, not GUI or full P6'}
try:
 call([str(S/'setup.exe'),'/S','/NS','/D='+str(I)]);x=I/'cerulean-runtime.exe';core('service','start');core('project','add','--id','candidate-overwrite-20261013','--root',P,'--timezone','Asia/Shanghai');before=core('project','show','--project','candidate-overwrite-20261013')['result']['workflow'];save('project-before.json',before)
 q=core('upgrade','review','--source-package',S,'--source-manifest-sha256',h(S/'manifest.json'),'--candidate-package',C,'--candidate-manifest-sha256',h(C/'manifest.json'));hold=core('upgrade','prepare','--review-digest',q['result']['upgradeReview']['digest'])['result']['upgradeHold']['holdDigest'];save('review-hold.json',{'review':q,'holdDigest':hold})
 args=['-Action','Prepare','-TransactionDirectory',T,'-InstalledDirectory',I,'-DataDirectory',D,'-HoldDigest',hold,'-SourcePackage',S,'-SourceManifestSha256',h(S/'manifest.json'),'-CandidatePackage',C,'-CandidateManifestSha256',h(C/'manifest.json'),'-AllowUnsignedInternal'];ps(*args);counts['Prepare']=1;old=inv(I);od=inv(D);save('original-program.json',old);save('original-data.json',od)
 k=ctypes.windll.kernel32;k.CreateFileW.restype=ctypes.c_void_p;g=k.CreateFileW(str(x.resolve()),0xC0000000,3,None,3,0x80,None)
 if g in (None,ctypes.c_void_p(-1).value):raise OSError('CreateFileW failed')
 save('shared-lock.json',{'path':str(x),'handle':int(g),'share':'READ|WRITE no DELETE'})
 try:ps('-Action','Apply','-TransactionDirectory',T,'-PlanDigest',h(T/'plan.json'),'-InstalledDirectory',I,'-DataDirectory',D,'-CandidatePackage',C,'-CandidateManifestSha256',h(C/'manifest.json'),'-AllowUnsignedInternal',e=1);counts['ApplyRejected']=1
 finally:k.CloseHandle(ctypes.c_void_p(g));save('shared-lock-released.json',{'released':True})
 ps('-Action','Apply','-TransactionDirectory',T,'-PlanDigest',h(T/'plan.json'),'-InstalledDirectory',I,'-DataDirectory',D,'-CandidatePackage',C,'-CandidateManifestSha256',h(C/'manifest.json'),'-AllowUnsignedInternal');counts['ApplyContinuation']=1
 ps('-Action','Commit','-TransactionDirectory',T,'-PlanDigest',h(T/'plan.json'),'-InstalledDirectory',I,'-DataDirectory',D,'-CandidatePackage',C,'-CandidateManifestSha256',h(C/'manifest.json'),'-AllowUnsignedInternal');counts['Commit']=1
 new=inv(I);save('program-after.json',new);save('data-after.json',inv(D));r.update(status='passed',counts=counts,actualFault='CreateFileW no-delete share rejected Apply',sameTransactionContinuation=True,originalDataUnchanged=inv(D)==od,candidatePublished=new!=old,planDigest=h(T/'plan.json'))
except Exception as e:r.update(error=repr(e),counts=counts,preservedFailure=True)
finally:r['calls']=calls;save('result.json',r);print(json.dumps(r,ensure_ascii=False))
if r['status']!='passed':raise SystemExit(1)
