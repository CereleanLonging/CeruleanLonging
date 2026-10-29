import json,hashlib,subprocess,pathlib,datetime,time,shutil,os
R=pathlib.Path(__file__).resolve().parent.parent
O=R/'reports/staged-program-interrupt-20261013T023654253';N=R/'reports'/('staging-resume-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S%f'));N.mkdir()
I=O/'installed app';D=O/'data';P=O/'project';T=O/'transaction';S=R/'reports/p6-windows-package-017-20261012';C=R/'reports/p6-windows-package-018-20261012';F=R/'reports/recovery-window-native-3fd2cbd6c2d8424e8ea79d606baf6dba/maintenance';U=F/'manage-windows-update.ps1';B=I/'cerulean-runtime.exe';calls=[];counts={}
def save(n,v):(N/n).write_text(json.dumps(v,ensure_ascii=False,indent=2),encoding='utf-8')
def ext(p):return '\\\\?\\'+str(p.resolve())
def h(p):
 with open(ext(p),'rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def inv(p):
 root=ext(p);a=[]
 for d,ds,fs in os.walk(root):
  for n in fs:
   x=os.path.join(d,n)
   with open(x,'rb') as f:z=hashlib.file_digest(f,'sha256').hexdigest()
   a.append({'path':os.path.relpath(x,root),'bytes':os.stat(x).st_size,'sha256':z})
 return sorted(a,key=lambda x:x['path'])
def run(a,reject=False):
 a=list(map(str,a));n=f'call-{len(calls)+1:03d}';save(n+'.request.json',a);p=subprocess.Popen(a,stdout=subprocess.PIPE,stderr=subprocess.PIPE);out,err=p.communicate(timeout=120);(N/(n+'.stdout')).write_bytes(out);(N/(n+'.stderr')).write_bytes(err);v={'args':a,'pid':p.pid,'exit':p.returncode,'reject':reject};calls.append(v);save(n+'.receipt.json',v)
 if reject:assert p.returncode!=0;return
 assert p.returncode==0,err.decode('utf-8','replace')
 return json.loads(out.decode('utf-8-sig')) if out.strip() else None
def ps(s):return run(['powershell.exe','-NoProfile','-Command',s])
def core(*a,reject=False):return run([B,'--workbench','--data-dir',D,*a],reject)
def audit(n):
 v=ps("Get-CimInstance Win32_Process | Where-Object {$_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.Contains('"+str(O)+"') -and $_.Name -in @('cerulean-runtime.exe','cerulean-core.exe','powershell.exe')} | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine,CreationDate | ConvertTo-Json -Depth 5 -Compress");save(n,v);return v
result={'status':'failed','run':str(N),'originalRun':str(O),'cleanup':'preserved-for-inspection'}
try:
 shutil.copyfile(R/'scripts/check-staged-program-interrupt-20261013.ps1',N/'original-failed-driver.ps1');shutil.copyfile(__file__,N/'continuation-driver.py');save('original-failure.json',json.loads((O/'result.json').read_text(encoding='utf-8-sig')))
 session=json.loads((F/'session.json').read_text(encoding='utf-8-sig'))
 for x in session['artifacts']:assert h(F/x['path'])==x['sha256']
 save('tool-identity.json',{'session':session,'sourceManifest':h(S/'manifest.json'),'candidateManifest':h(C/'manifest.json')});audit('initial-processes.json')
 reg=ps("@{install=(Get-Item 'HKCU:\\Software\\ceruleanlonging\\CeruleanLonging').GetValue('');version=(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CeruleanLonging').DisplayVersion}|ConvertTo-Json");save('registration-before.json',reg);assert reg['install'].lower()==str(I).lower() and reg['version']=='0.1.7';assert not T.exists()
 snapshot=core('status');save('snapshot-before.json',snapshot);assert not snapshot['result']['workflows'] and snapshot['upgradeReview'] is None
 core('project','add','--id','staging-20261013','--root',str(P),'--timezone','Asia/Shanghai');original=core('project','show','--project','staging-20261013')['result']['workflow'];save('project-before.json',original)
 review=core('upgrade','review');hold=core('upgrade','prepare','--review-digest',review['upgradeReview']['digest'])['upgradeHold']['holdDigest']
 fixture=I/'000-prepare-stage-fixture.bin'
 with open(fixture,'xb') as f:
  block=b'ACCEPTANCE 118 PROGRAM RESOURCE FIXTURE\n'.ljust(1048576,b'\0')
  for _ in range(256):f.write(block)
  f.flush();os.fsync(f.fileno())
 program=inv(I);data=inv(D);save('program-before.json',program);save('data-before.json',data)
 args=['powershell.exe','-NoProfile','-ExecutionPolicy','RemoteSigned','-File',str(U),'-Action','Prepare','-TransactionDirectory',str(T),'-InstalledDirectory',str(I),'-DataDirectory',str(D),'-HoldDigest',hold,'-SourcePackage',str(S),'-SourceManifestSha256',h(S/'manifest.json'),'-CandidatePackage',str(C),'-CandidateManifestSha256',h(C/'manifest.json'),'-AllowUnsignedInternal'];save('prepare.request.json',args)
 p=subprocess.Popen(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE);counts['Prepare']=1;save('owned-prepare.json',{'pid':p.pid,'parentPid':os.getpid(),'handle':int(p._handle),'args':args});staged=T/'staged-program'/fixture.name;hit=None;deadline=time.monotonic()+120
 while p.poll() is None and time.monotonic()<deadline:
  if staged.exists():
   amount=staged.stat().st_size
   if 0<amount<268435456 and (T/'data-backup/manifest.json').exists():hit={'path':str(staged),'bytes':amount,'sourceBytes':268435456,'backupManifest':h(T/'data-backup/manifest.json'),'alive':p.poll() is None};break
  time.sleep(.001)
 if not hit:
  out,err=p.communicate(timeout=120);(N/'prepare.stdout').write_bytes(out);(N/'prepare.stderr').write_bytes(err);raise AssertionError('Partial staging copy not hit; original Prepare not repeated')
 save('termination-observation.json',hit);p.terminate();out,err=p.communicate(timeout=60);(N/'prepare.stdout').write_bytes(out);(N/'prepare.stderr').write_bytes(err);save('prepare.receipt.json',{'pid':p.pid,'exit':p.returncode,'waited':True,'stdoutEOF':True,'stderrEOF':True});assert p.returncode!=0;assert not audit('processes-after-termination.json');assert not (T/'plan.json').exists() and not list(T.glob('event-*.json'));assert 0<staged.stat().st_size<268435456
 verify=run([C/'cerulean-runtime.exe','--workbench','--data-dir',D,'backup','verify','--from',T/'data-backup']);save('backup-verification.json',verify);assert inv(I)==program and inv(D)==data;residual=inv(T);save('residual-before.json',residual)
 intent=h(T/'intent.json');guard=json.loads((T/'maintenance-reservation.json').read_text(encoding='utf-8-sig'))['guardDigest'];save('transaction-identity.json',{'intent':intent,'guard':guard,'hold':hold})
 core('service','start',reject=True);core('upgrade','release','--hold-digest',hold,reject=True)
 def abandon():return run(['powershell.exe','-NoProfile','-ExecutionPolicy','RemoteSigned','-File',U,'-Action','AbandonPreparation','-TransactionDirectory',T,'-IntentDigest',intent,'-GuardDigest',guard,'-AllowUnsignedInternal'])
 a=abandon();counts['AbandonPreparation']=1;assert a['maintenance']['state']=='released' and a['holdPresent'];assert inv(T)==residual and inv(D)==data and inv(I)==program
 a=abandon();counts['archivedReadback']=1;assert a['maintenance']['state']=='released';assert inv(T)==residual
 core('upgrade','release','--hold-digest',hold);counts['holdReleaseSuccess']=1;core('service','start');assert core('project','show','--project','staging-20261013')['result']['workflow']==original;core('service','stop')
 un=N/'owned-uninstall.exe';shutil.copyfile(I/'uninstall.exe',un);run([un,'/S','_?='+str(I)])
 clean=ps("$u='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CeruleanLonging';$k='HKCU:\\Software\\ceruleanlonging\\CeruleanLonging';if(Test-Path $k){if((Get-Item $k).GetValue('') -ine '"+str(I)+"'){throw 'Ownership differs'};Remove-Item $k -Recurse};@{uninstallPresent=(Test-Path $u);productPresent=(Test-Path $k)}|ConvertTo-Json");assert not clean['uninstallPresent'] and not clean['productPresent'];assert not audit('final-processes.json');assert not (D/'upgrade-hold.json').exists();save('residual-after.json',inv(T));assert inv(T)==residual
 result.update(status='passed',cleanup='passed',realPartialStagingObservation=hit,backupVerified=True,originalProgramAndDataUnchanged=True,residualPreserved=True,planPublished=False,preparedEventPublished=False,projectReadbackUnchanged=True,registration=clean,scope='direct CLI/PowerShell isolated installation, not GUI or full P6')
except Exception as e:result['error']=repr(e)
finally:result['calls']=calls;result['counts']=counts;save('result.json',result);print(json.dumps(result,ensure_ascii=False))
if result['status']!='passed':raise SystemExit(1)
