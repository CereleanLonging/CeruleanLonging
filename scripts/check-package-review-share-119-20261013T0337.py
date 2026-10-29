import ctypes,hashlib,json,subprocess,shutil
from datetime import datetime
from pathlib import Path
R=Path(__file__).resolve().parent.parent;src=R/'reports/p6-windows-package-018-20261012';run=R/'reports'/('package-review-share-119-'+datetime.now().strftime('%Y%m%dT%H%M%S%f'));pkg=run/'candidate';run.mkdir(parents=True);shutil.copytree(src,pkg);calls=[]
def sha(p):
 h=hashlib.sha256();f=open('\\\\?\\'+str(Path(p).resolve()),'rb')
 for b in iter(lambda:f.read(1048576),b''):h.update(b)
 f.close();return h.hexdigest()
def save(n,v): (run/n).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
manifest=sha(pkg/'manifest.json');arts={x:sha(pkg/x) for x in ['setup.exe','CeruleanLonging.exe','cerulean-runtime.exe']};target=pkg/'cerulean-runtime.exe';targetHash=sha(target);targetBytes=target.stat().st_size;save('package-identity.json',{'package':str(pkg),'manifestSha256':manifest,'artifacts':arts})
def review(expect):
 a=['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(R/'scripts/test-windows-package.ps1'),'-PackageDirectory',str(pkg),'-ManifestSha256',manifest,'-AllowUnsignedInternal'];p=subprocess.Popen(a,stdout=subprocess.PIPE,stderr=subprocess.PIPE);o,e=p.communicate(timeout=180);r={'args':a,'pid':p.pid,'exit':p.returncode,'waited':True,'stdoutEOF':True,'stderrEOF':True,'stdout':o.decode('utf-8-sig','replace'),'stderr':e.decode('utf-8-sig','replace')};calls.append(r);save('call-%03d.json'%len(calls),r)
 if p.returncode!=expect:raise RuntimeError(r)
k=ctypes.windll.kernel32;k.CreateFileW.restype=ctypes.c_void_p;h=k.CreateFileW(str(target),0xC0000000,0,None,3,0x80,None)
if h in (None,ctypes.c_void_p(-1).value):raise OSError('CreateFileW failed')
save('shared-lock.json',{'path':str(target),'access':'GENERIC_READ|GENERIC_WRITE','share':'NONE','handle':int(h),'sha256':targetHash,'bytes':targetBytes})
try:review(1)
finally:k.CloseHandle(ctypes.c_void_p(h));save('shared-lock-released.json',{'released':True})
review(0);result={'status':'passed','run':str(run),'scenario':'candidate-package-review-shared-executable-rejection','actualFault':'CreateFileW FILE_SHARE_NONE on copied candidate cerulean-runtime.exe rejected public package review','firstRejectedExit':calls[0]['exit'],'secondReviewExit':calls[1]['exit'],'artifactSha256':arts,'manifestSha256':manifest,'installed':False,'dataOpened':False,'modelCalls':0,'userDesktopActions':0,'acceptedScope':'direct public PowerShell package review; not Prepare/Apply/plan publication, GUI, or full P6'};save('result.json',result);print(json.dumps(result,ensure_ascii=False))
