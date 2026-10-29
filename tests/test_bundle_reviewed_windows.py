"""Reviewed byte-boundary tests; fixtures are not signed executables."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
spec=importlib.util.spec_from_file_location('bundle_reviewed',ROOT/'scripts/bundle-reviewed-windows.py')
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)

class Inputs(unittest.TestCase):
    def setUp(self):
        base=ROOT/'target/bundle-reviewed-input-tests'
        base.mkdir(parents=True,exist_ok=True)
        self.temp=tempfile.TemporaryDirectory(prefix='bundle-input-test-',dir=base)
        self.root=Path(self.temp.name)
        self.assertTrue(self.root.resolve().is_relative_to(base.resolve()))
        self.addCleanup(self.temp.cleanup)
        self.records=[]
        for name in b.NAMES:
            body=(b'MZfixture\0'+b.MARKER+b'NSS\0opaque-overlay-preserved') if name=='CeruleanLonging.exe' else b'fixture-bytes'
            (self.root/name).write_bytes(body)
            self.records.append({'path':name,'bytes':len(body),'sha256':hashlib.sha256(body).hexdigest()})
        self.manifest={'schema':1,'kind':'reviewed-windows-bundle-inputs','product':'CeruleanLonging','identifier':'local.ceruleanlonging.workbench','version':'0.1.27','files':self.records}
    def publish(self):
        raw=json.dumps(self.manifest).encode();(self.root/'input-manifest.json').write_bytes(raw)
        return hashlib.sha256(raw).hexdigest()
    def test_nss_and_overlay_preserved_without_rewrite(self):
        before=(self.root/'CeruleanLonging.exe').read_bytes()
        b.validate_inputs(self.root,self.publish())
        self.assertEqual((self.root/'CeruleanLonging.exe').read_bytes(),before)
    def test_unknown_marker_rejected_before_signing_preservation(self):
        file=self.root/'CeruleanLonging.exe';file.write_bytes(file.read_bytes().replace(b'NSS',b'UNK'))
        self.records[0]['sha256']=b.H.digest(file)
        with self.assertRaisesRegex(ValueError,'BEFORE signing'):b.validate_inputs(self.root,self.publish())
    def test_duplicate_marker_rejected(self):
        file=self.root/'CeruleanLonging.exe';file.write_bytes(file.read_bytes()+b.MARKER+b'NSS')
        self.records[0].update(bytes=file.stat().st_size,sha256=b.H.digest(file))
        with self.assertRaisesRegex(ValueError,'Unique PE'):b.validate_inputs(self.root,self.publish())
    def test_payload_change_rejected(self):
        digest=self.publish();file=self.root/'cerulean-runtime.exe';file.write_bytes(b'other-content')
        with self.assertRaisesRegex(ValueError,'Payload bytes changed'):b.validate_inputs(self.root,digest)
    def test_manifest_substitution_rejected(self):
        self.publish()
        with self.assertRaisesRegex(ValueError,'manifest changed'):b.validate_inputs(self.root,'0'*64)
    def test_traversal_name_rejected(self):
        self.records[-1]['path']='../THIRD-PARTY-MANIFEST.json'
        with self.assertRaisesRegex(ValueError,'seven payloads'):b.validate_inputs(self.root,self.publish())
    def test_duplicate_resource_rejected(self):
        self.records[-1]=dict(self.records[-2])
        with self.assertRaisesRegex(ValueError,'seven payloads'):b.validate_inputs(self.root,self.publish())

if __name__=='__main__':unittest.main()
