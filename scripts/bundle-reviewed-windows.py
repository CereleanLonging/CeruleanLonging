"""Bundle reviewed Windows payload bytes without compiling, patching or signing.

The GUI must already contain the NSS bundle marker before it is signed. Input
hashes identify reviewed bytes; they are NOT a certificate or provenance check.
The unsigned outer installer still requires separate signing and verification.
"""
import argparse
from contextlib import contextmanager, ExitStack
from datetime import datetime, timezone
import ctypes
from ctypes import wintypes as W
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

import isolated_nsis_install as H
import observe_owned_process as observer
import review_nsis_payload as payload

ROOT=Path(__file__).resolve().parent.parent
NAMES=('CeruleanLonging.exe','cerulean-runtime.exe',*H.RESOURCE_LIMITS)
MARKER=b'TAURI_BUNDLE_TYPE_VAR_'

def require(value,message):
    if not value:raise ValueError(message)

def read(path):
    H.no_reparse(path)
    require(path.stat().st_size<=1024*1024,'Metadata exceeds bound')
    return json.loads(path.read_text(encoding='utf-8-sig'))

def validate_inputs(directory,digest):
    require(re.fullmatch('[a-f0-9]{64}',digest) is not None,'Exact input-manifest digest required')
    path=directory/'input-manifest.json'
    require(H.digest(path)==digest,'Reviewed input manifest changed')
    manifest=read(path)
    require(manifest.get('schema')==1 and manifest.get('kind')=='reviewed-windows-bundle-inputs'
            and manifest.get('product')=='CeruleanLonging'
            and manifest.get('identifier')=='local.ceruleanlonging.workbench','Wrong payload identity')
    require(re.fullmatch(r'0\.1\.\d+',manifest.get('version','')) is not None,'Unsupported version')
    require(tuple(map(int,manifest['version'].split('.'))) < (0,1,39),
            'This historical signing helper handles the seven-file layout only; build the bound independent verifier with scripts/desktop.mjs package')
    files=manifest.get('files',[])
    require(len(files)==len(NAMES) and {r.get('path') for r in files}==set(NAMES),'Exact seven payloads required')
    for row in files:
        file=directory/row['path'];H.no_reparse(file)
        limit=128*1024*1024 if row['path'].endswith('.exe') else H.RESOURCE_LIMITS[row['path']]
        require(type(row.get('bytes')) is int and 0<row['bytes']<=limit,'Payload size declaration invalid')
        require(file.stat().st_size==row['bytes'] and H.digest(file)==row.get('sha256'),'Payload bytes changed: '+row['path'])
    image=(directory/'CeruleanLonging.exe').read_bytes()
    require(image.startswith(b'MZ') and image.count(MARKER)==1,'Unique PE bundle marker required')
    position=image.index(MARKER)+len(MARKER)
    require(image[position:position+3]==b'NSS','GUI must be normalized to NSS BEFORE signing; this entry never patches bytes')
    return manifest

def check_capacity(path,digest):
    require(H.digest(path)==digest,'Capacity receipt changed')
    value=read(path)
    age=(datetime.now(timezone.utc)-datetime.fromisoformat(value['at'])).total_seconds()
    require(value.get('passed') is True and value.get('limitBytes')==120_000_000_000
            and value.get('reserveBytes',0)>=1024**3 and not value.get('errors')
            and 0<=age<=1800,'Fresh passing 1 GiB capacity receipt required')
    require(shutil.disk_usage(ROOT).free>=1024**3+10_000_000_000,'Actual disk floor unavailable')

@contextmanager
def pinned(path):
    H.no_reparse(path)
    k=ctypes.WinDLL('kernel32',use_last_error=True)
    k.CreateFileW.argtypes=[W.LPCWSTR,W.DWORD,W.DWORD,ctypes.c_void_p,W.DWORD,W.DWORD,W.HANDLE]
    k.CreateFileW.restype=W.HANDLE
    k.CloseHandle.argtypes=[W.HANDLE];k.CloseHandle.restype=W.BOOL
    handle=k.CreateFileW(str(path),0x80000000,1,None,3,0x00200000,None)
    if handle==ctypes.c_void_p(-1).value:raise ctypes.WinError(ctypes.get_last_error())
    try:yield
    finally:
        if not k.CloseHandle(handle):raise ctypes.WinError(ctypes.get_last_error())

def copy_new(source,destination):
    destination.parent.mkdir(parents=True,exist_ok=True)
    with source.open('rb') as src,destination.open('xb') as dst:shutil.copyfileobj(src,dst)
    require(H.digest(source)==H.digest(destination),'Staged copy differs')

def execute(args):
    require(os.name=='nt' and not sys.flags.optimize,'Windows without Python -O required')
    directory=args.input_directory.absolute();run=args.run_directory.absolute()
    H.no_reparse(directory);H.no_reparse(run)
    require(run.parent==ROOT/'reports' and run.name.startswith('bundle-reviewed-') and not run.exists(),'Fresh direct reports/bundle-reviewed-* run required')
    require(not directory.is_relative_to(run) and not run.is_relative_to(directory),'Input and output overlap')
    check_capacity(args.storage_receipt,args.storage_sha256)
    manifest=validate_inputs(directory,args.input_manifest_sha256)
    config_path=ROOT/'apps/desktop/src-tauri/tauri.conf.json';config=read(config_path)
    require(config['version']==manifest['version'] and config['mainBinaryName']=='CeruleanLonging','Current metadata and input versions differ')
    require(config['bundle']['windows']['nsis']['installMode']=='currentUser'
            and config['bundle']['windows']['allowDowngrades'] is False,'Unexpected installation scope')
    require(not config['build'].get('beforeBundleCommand'),'Unexpected pre-bundle command')
    require(not list((ROOT/'apps/desktop/src-tauri').glob('tauri.windows.*')),'Review platform-specific configuration before using this entry')
    inputs=manifest.get('buildInputs',[])
    require(len(inputs)>0 and len({r['path'] for r in inputs})==len(inputs),'Build-input identities missing or duplicated')
    bindings={}
    for row in inputs:
        rel=Path(row['path'])
        require(not rel.is_absolute() and '..' not in rel.parts,'Unsafe build-input path')
        path=ROOT/rel;H.no_reparse(path)
        require(H.digest(path)==row['sha256'],'Reviewed source input changed: '+str(rel))
        bindings[str(path)]=row['sha256']
    require(str(config_path) in bindings and str(ROOT/'apps/desktop/src-tauri/installer/hooks.nsh') in bindings,'Configuration and NSIS hooks must be bound')
    by_name={r['path']:r for r in manifest['files']}
    for name in H.RESOURCE_LIMITS:
        path=ROOT/'third-party'/name
        require(H.digest(path)==by_name[name]['sha256'],'Current resource differs from reviewed payload')
        bindings[str(path)]=by_name[name]['sha256']
    for rel in config['bundle']['icon']:
        path=(config_path.parent/rel).resolve();require(path.is_relative_to(config_path.parent),'Icon escapes app source')
        bindings[str(path)]=H.digest(path)
    node=args.node.absolute();require(node.is_file(),'Explicit Node executable required')
    bindings[str(node)]=H.digest(node)
    env={k.upper():v for k,v in os.environ.items() if not any(s in k.upper() for s in ('KEY','TOKEN','SECRET','CREDENTIAL','AUTH','PASSWORD','SIGN')) and not k.upper().startswith(('CARGO_','RUST','TAURI_','CERULEAN_'))}
    env.update(CARGO_HOME=str(args.cargo_home.absolute()),RUSTUP_HOME=str(args.rustup_home.absolute()),CARGO_NET_OFFLINE='true',CI='true')
    env['PATH']=os.pathsep.join((str(args.cargo_home.absolute()/'bin'),str(node.parent),env.get('PATH','')))
    spec=importlib.util.spec_from_file_location('distribution_bundle',ROOT/'scripts/distribution-build-inputs.py')
    gate=importlib.util.module_from_spec(spec);spec.loader.exec_module(gate)
    checked=gate.check(ROOT,env=env)
    run.mkdir()
    H.save_new(run/'intent.json',{'at':datetime.now(timezone.utc).isoformat(),'inputManifestSha256':args.input_manifest_sha256,'driverSha256':H.digest(Path(__file__)),'bindings':bindings,'distributionGate':checked,'automaticRetry':False,'compilationRequested':False,'signingRequested':False,'publisherVerified':False,'releaseAccepted':False})
    def command(label,argv,cwd):
        folder=run/label;folder.mkdir()
        H.save_new(folder/'command.json',{'argv':argv,'cwd':str(cwd),'automaticRetry':False})
        class System:
            def start(self,command,executable,work):
                return subprocess.Popen(command,executable=str(executable),cwd=cwd,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
        result={}
        child=observer.observe_launch(System(),argv,Path(argv[0]),folder,300,lambda p:print(label+' PID '+str(p.pid),flush=True),result,H.save_new)
        record={k:result[k] for k in ('stdoutEOF','stderrEOF','initialTimeout','observationErrors')}
        record['exitCode']=child.returncode
        H.save_new(folder/'result.json',record)
        require(child.returncode==0 and record['stdoutEOF'] and record['stderrEOF'] and not record['initialTimeout'] and not record['observationErrors'],'Original '+label+' failed or unknown; no replay')
        return folder
    try:
        with ExitStack() as pins:
            for path in [directory/'input-manifest.json',*(directory/n for n in NAMES),*(Path(p) for p in bindings)]:pins.enter_context(pinned(path))
            validate_inputs(directory,args.input_manifest_sha256)
            target=run/'cargo-target';gui=target/'release/CeruleanLonging.exe'
            runtime=run/'inputs/cerulean-runtime-x86_64-pc-windows-msvc.exe'
            copy_new(directory/'CeruleanLonging.exe',gui);copy_new(directory/'cerulean-runtime.exe',runtime)
            override={'build':{'beforeBundleCommand':None},'bundle':{'externalBin':[str(runtime.with_name('cerulean-runtime'))]}}
            H.save_new(run/'bundle-config.json',override)
            env['CARGO_TARGET_DIR']=str(target)
            cargo=Path(env['CARGO_HOME'])/'bin/cargo.exe'
            metadata_folder=command('metadata',[str(cargo),'metadata','--locked','--offline','--no-deps','--format-version','1'],config_path.parent)
            metadata=read(metadata_folder/'installer.stdout')
            require(Path(metadata['target_directory'])==target,'Cargo did not select the owned staging target')
            argv=[str(node),str(ROOT/'node_modules/@tauri-apps/cli/tauri.js'),'bundle','--ci','--bundles','nsis','--no-sign','--no-binary-patching','--config',str(run/'bundle-config.json')]
            command('bundle',argv,config_path.parent)
            require(H.digest(gui)==by_name['CeruleanLonging.exe']['sha256'] and H.digest(runtime)==by_name['cerulean-runtime.exe']['sha256'],'Bundler changed staged payload bytes')
            setup=target/'release/bundle/nsis'/('CeruleanLonging_'+manifest['version']+'_x64-setup.exe')
            require(setup.is_file(),'Expected installer not produced in owned target')
            archive=run/'archive';archive.mkdir()
            copy_new(setup,archive/'setup.exe')
            for name in NAMES:copy_new(directory/name,archive/name)
            artifact=lambda name:{'path':name,'bytes':(archive/name).stat().st_size,'sha256':H.digest(archive/name)}
            frozen={'schema':1,'product':manifest['product'],'identifier':manifest['identifier'],'version':manifest['version'],'channel':'packaging-byte-preservation-check','releaseAccepted':False,'publisherVerified':False,'signatureVerification':'not-performed; input hashes do not establish trusted signatures','artifacts':[artifact(n) for n in ('setup.exe','CeruleanLonging.exe','cerulean-runtime.exe')],'resources':[artifact(n) for n in H.RESOURCE_LIMITS]}
            H.save_new(archive/'manifest.json',frozen)
            reviewed=payload.review(archive,H.digest(archive/'manifest.json'))
            H.save_new(run/'payload-review.json',reviewed)
            validate_inputs(directory,args.input_manifest_sha256)
            require(all(H.digest(Path(p))==h for p,h in bindings.items()),'Original source/tool inputs changed')
        H.save_new(run/'result.json',{'status':'reviewed-payload-bytes-preserved','inputManifestSha256':args.input_manifest_sha256,'archiveManifestSha256':H.digest(archive/'manifest.json'),'verifiedPayloads':[r['name'] for r in reviewed['verified']],'inputAndStagedBytesUnchanged':True,'compilationRequested':False,'binaryPatchingRequested':False,'signingRequested':False,'installerExecuted':False,'publisherVerified':False,'releaseAccepted':False,'formalAcceptance':False})
        print('Reviewed payload bytes preserved; unsigned outer installer requires separate signing verification')
    except BaseException as exc:
        H.save_new(run/'failure.json',{'type':type(exc).__name__,'message':str(exc),'automaticRetry':False,'releaseAccepted':False})
        raise

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--execute',action='store_true',required=True)
    p.add_argument('--input-directory',type=Path,required=True)
    p.add_argument('--input-manifest-sha256',required=True)
    p.add_argument('--run-directory',type=Path,required=True)
    p.add_argument('--storage-receipt',type=Path,required=True)
    p.add_argument('--storage-sha256',required=True)
    p.add_argument('--node',type=Path,required=True)
    p.add_argument('--cargo-home',type=Path,default=Path.home()/'.cargo')
    p.add_argument('--rustup-home',type=Path,default=Path.home()/'.rustup')
    execute(p.parse_args())

if __name__=='__main__':main()
