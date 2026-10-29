"""Local archive boundary tests; never open workbench data or execute payloads."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('portable',ROOT/'scripts/portable-evidence.py')
p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.area=Path(self.temp.name);self.source=self.area/'source';self.source.mkdir()
        (self.source/'evidence.json').write_bytes(b'{"realityAttested":false}\n')
        (self.source/'logs').mkdir();(self.source/'logs/result.txt').write_bytes(b'preserved unknown\n')
        # This unselected file must never be read or included.
        (self.source/'credentials').mkdir();(self.source/'credentials/private.dpapi').write_bytes(b'unselected trap')
        self.index=self.area/'index.json'
        self.entries=[dict(path=name,bytes=(self.source/name).stat().st_size,sha256=p.sha((self.source/name).read_bytes())) for name in ['evidence.json','logs/result.txt']]
        self.expected=self.save_index(self.entries)
    def save_index(self, entries):
        raw=p.encode(entries);self.index.write_bytes(raw);return p.sha(raw)
    def make(self, name='archive'):
        archive=self.area/name
        result=p.create(self.source,self.index,self.expected,archive)
        return archive,result['manifestSha256']
    def test_relocation_no_source_and_no_overwrite(self):
        archive,digest=self.make()
        moved=self.area/'relocated';shutil.copytree(archive,moved)
        self.source.rename(self.area/'source-unavailable');self.index.unlink()
        result=p.verify(moved,digest)
        self.assertEqual(result['filesVerified'],2);self.assertFalse(result['sourceWorkspaceRequired'])
        self.assertFalse(result['executionAllowed']);self.assertFalse(result['realityAttested'])
        self.assertFalse((moved/'payload/credentials').exists())
        with self.assertRaises(ValueError):p.verify(moved,'0'*64)
        original=(archive/'manifest.json').read_bytes()
        with self.assertRaises(ValueError):p.create(self.area/'source-unavailable',self.area/'missing-index',self.expected,archive)
        self.assertEqual((archive/'manifest.json').read_bytes(),original)
    def test_tamper_missing_extra_and_pending_are_refused(self):
        archive,digest=self.make()
        for case in ['changed','missing','extra','extra-directory','pending','manifest']:
            copy=self.area/case;shutil.copytree(archive,copy)
            if case=='changed':(copy/'payload/logs/result.txt').write_bytes(b'false success\n')
            if case=='missing':(copy/'payload/evidence.json').unlink()
            if case=='extra':(copy/'payload/extra.txt').write_text('not selected')
            if case=='extra-directory':(copy/'payload/extra').mkdir()
            if case=='pending':(copy/'manifest.pending').write_text('unfinished')
            if case=='manifest':(copy/'manifest.json').write_text('{}')
            with self.subTest(case=case),self.assertRaises((ValueError,OSError)):p.verify(copy,digest)
    def test_index_scope_credentials_duplicates_and_unsupported_claims(self):
        for number,name in enumerate(['../escape','/absolute','C:/absolute','a\\b','a/../b','a//b','a/./b','file:stream','CON.txt','logs/trailing.','.env','credentials/private.dpapi','key.pem']):
            bad=[dict(path=name,bytes=0,sha256=p.sha(b''))];expected=self.save_index(bad);dest=self.area/f'bad-{number}'
            with self.subTest(name=name),self.assertRaises(ValueError):p.create(self.source,self.index,expected,dest)
            self.assertFalse(dest.exists())
        for bad in [[*self.entries,self.entries[0]], [self.entries[0],dict(self.entries[0],path='EVIDENCE.JSON')], [dict(self.entries[0],bytes=True)], [dict(self.entries[0],path='logs'),self.entries[1]]]:
            with self.assertRaises(ValueError):p.inventory(bad)
        with self.assertRaises(ValueError):p.parse(b'{"a":1,"a":2}')
        self.expected=self.save_index(self.entries)
        archive,digest=self.make()
        raw=p.parse((archive/'manifest.json').read_bytes());raw['executionAllowed']=True
        changed=p.encode(raw);(archive/'manifest.json').write_bytes(changed)
        with self.assertRaises(ValueError):p.verify(archive,p.sha(changed))
        with self.assertRaises(ValueError):p.create(self.source,self.index,self.expected,self.source/'nested')
    def test_failed_copy_retains_incomplete_archive_and_original(self):
        original=(self.source/'evidence.json').read_bytes();self.entries[1]['sha256']='0'*64
        self.expected=self.save_index(self.entries);dest=self.area/'partial'
        with self.assertRaises(ValueError):p.create(self.source,self.index,self.expected,dest)
        self.assertTrue((dest/'manifest.pending').exists());self.assertFalse((dest/'manifest.json').exists())
        self.assertEqual((self.source/'evidence.json').read_bytes(),original)
        with self.assertRaises(ValueError):p.verify(dest,'0'*64)
        with self.assertRaises(ValueError):p.create(self.source,self.index,self.expected,dest)
    def test_hardlinks_and_directory_junctions_refused(self):
        alias=self.area/'alias';os.link(self.source/'evidence.json',alias)
        with self.assertRaises(ValueError):self.make('hardlink')
        alias.unlink()
        if os.name=='nt':
            link=self.area/'junction'
            result=subprocess.run(['cmd.exe','/c','mklink','/J',str(link),str(self.source)],capture_output=True,creationflags=0x08000000,check=False)
            self.assertEqual(result.returncode,0)
            try:
                with self.assertRaises(ValueError):p.create(link,self.index,self.expected,self.area/'junction-archive')
            finally:link.rmdir()  # only this fixture junction, never its target
            with p.windows_pin(self.source/'evidence.json',False):
                with self.assertRaises(OSError):(self.source/'evidence.json').write_bytes(b'blocked mutation')


if __name__=='__main__':unittest.main(verbosity=2)
