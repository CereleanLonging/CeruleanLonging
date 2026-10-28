"""Generate/check the SQLite dependency notices from checksum-verified local crates."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
NAMES = ('rusqlite', 'libsqlite3-sys', 'fallible-iterator', 'fallible-streaming-iterator', 'bitflags', 'smallvec')
TEXT = ROOT / 'third-party/SQLITE-NOTICES.txt'
INDEX = ROOT / 'third-party/sqlite-notices.json'

def digest(data):
    return hashlib.sha256(data).hexdigest()

def generated():
    locked = tomllib.loads((ROOT/'Cargo.lock').read_text(encoding='utf-8'))['package']
    rusqlite = next(p for p in locked if p['name'] == 'rusqlite')
    dependencies = rusqlite['dependencies']
    selected = [rusqlite]
    for name in NAMES[1:]:
        declared = next(value for value in dependencies if value.split()[0] == name)
        matches = [p for p in locked if p['name'] == name and (len(declared.split()) == 1 or p['version'] == declared.split()[1])]
        assert len(matches) == 1, 'Ambiguous locked dependency: ' + name
        selected.extend(matches)
    registry = Path(os.environ.get('CARGO_HOME', Path.home()/'.cargo'))/'registry/src'
    sections = ['CeruleanLonging - SQLite component notices\n\nThis file covers the bundled SQLite engine, rusqlite, and its enabled direct runtime dependencies. Other product dependencies are outside this file. Upstream license texts follow unchanged.\n']
    records=[]
    sqlite=None
    for package in selected:
        matches = sorted(registry.glob(f"*/{package['name']}-{package['version']}"))
        assert len(matches) == 1, 'Missing or ambiguous local crate: ' + package['name']
        folder=matches[0]
        archive=registry.parent/'cache'/folder.parent.name/(folder.name+'.crate')
        with archive.open('rb') as stream:
            assert hashlib.file_digest(stream,'sha256').hexdigest() == package['checksum'], 'Cached crate checksum changed'
        def checked(relative):
            data=(folder/relative).read_bytes()
            with tarfile.open(archive,'r:gz') as bundle:
                member=bundle.getmember(folder.name+'/'+relative)
                assert member.isfile() and member.size<=32*1024*1024,'Unsupported crate member'
                assert data==bundle.extractfile(member).read(),'Crate source differs from locked archive: '+relative
            return data
        metadata=tomllib.loads(checked('Cargo.toml').decode('utf-8'))['package']
        license_files=sorted(p.name for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','NOTICE','COPYRIGHT')))
        assert license_files, package['name']
        entry={'name':package['name'],'version':package['version'],'crateChecksum':package['checksum'],
               'license':metadata.get('license'),'repository':metadata.get('repository'),'files':{}}
        sections.append(f"\n{'='*72}\n{entry['name']} {entry['version']}\nLicense: {entry['license']}\nRepository: {entry['repository']}\n")
        for name in license_files:
            data=checked(name)
            entry['files'][name]=digest(data)
            sections.append(f'\n--- {name} ---\n'+data.decode('utf-8'))
        if package['name']=='libsqlite3-sys':
            amalgamation=checked('sqlite3/sqlite3.c')
            version=re.search(rb'#define SQLITE_VERSION\s+"([^"]+)"',amalgamation).group(1).decode('ascii')
            begin=amalgamation.index(b'/*\n** 2001 September 15')
            end=amalgamation.index(b'*/',begin)+2
            notice=amalgamation[begin:end]
            assert b'The author disclaims copyright' in notice
            sqlite={'version':version,'sourceSha256':digest(amalgamation),'sourceCrate':entry['name'],
                    'sourceFile':'sqlite3/sqlite3.c','noticeSha256':digest(notice)}
            sections.append(f'\nSQLite {version} - source copyright disclaimer\n\n'+notice.decode('utf-8')+'\n')
        records.append(entry)
    assert sqlite
    text=''.join(sections).encode('utf-8')
    index={'schema':1,'scope':'SQLite engine and enabled rusqlite direct runtime dependencies; not full product SBOM',
           'noticePath':'third-party/SQLITE-NOTICES.txt','noticeSha256':digest(text),'packages':records,'sqlite':sqlite}
    return text,(json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode('utf-8')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check',action='store_true')
    args=parser.parse_args()
    text,index=generated()
    if args.check:
        assert TEXT.read_bytes()==text and INDEX.read_bytes()==index,'SQLite notices are stale; regenerate and review'
    else:
        TEXT.parent.mkdir(exist_ok=True)
        for path,data in [(TEXT,text),(INDEX,index)]:
            with path.open('xb') as stream: stream.write(data)
    print(json.dumps({'sqliteNotices':'verified' if args.check else 'generated','sha256':digest(text),'bytes':len(text),'packages':len(NAMES)}))
