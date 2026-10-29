"""Validate real distribution outputs against raw source materials, offline."""
import importlib.util
import io
import json
from pathlib import Path
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('distribution_notices', ROOT / 'scripts/distribution-notices.py')
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)


class DistributionNoticesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.outputs = generator.generated()
        cls.meta = json.loads((ROOT / 'third-party/distribution-inputs.json').read_text('utf-8'))

    def test_every_notice_and_required_source_is_preserved_byte_for_byte(self):
        with zipfile.ZipFile(ROOT / 'third-party/distribution-inputs.zip') as original, \
                zipfile.ZipFile(io.BytesIO(self.outputs['THIRD-PARTY-SOURCES.zip'])) as distributed:
            notice_names = {n for c in self.meta['components'] for n in c['notices']}
            for name in notice_names:
                self.assertEqual(original.read(name), distributed.read('original-notices/' + name), name)
            for name in self.meta['sourceFiles']:
                self.assertEqual(original.read(name), distributed.read(name), name)
            self.assertIn(b'CPL-1.0', distributed.read('SOURCE-ACCESS.txt'))

    def test_reviewed_encoding_changes_only_rendered_text(self):
        name = 'native/nsis/Contrib/Modern UI 2/License.txt'
        with zipfile.ZipFile(ROOT / 'third-party/distribution-inputs.zip') as original:
            data = original.read(name)
        self.assertIn(b'Copyright \xa9 2002-2025', data)
        text = self.outputs['THIRD-PARTY-NOTICES.txt'].decode('utf-8')
        self.assertIn(data.decode('latin-1'), text)
        manifest = json.loads(self.outputs['THIRD-PARTY-MANIFEST.json'])
        self.assertEqual(manifest['textEncodings'][name], 'latin-1')

    def test_output_manifest_covers_resources_and_each_zip_member(self):
        manifest = json.loads(self.outputs['THIRD-PARTY-MANIFEST.json'])
        for entry in manifest['files']:
            self.assertEqual(generator.sha(self.outputs[entry['path']]), entry['sha256'])
        with zipfile.ZipFile(io.BytesIO(self.outputs['THIRD-PARTY-SOURCES.zip'])) as bundle:
            self.assertEqual(set(bundle.namelist()), {e['path'] for e in manifest['sourceArchiveEntries']})
            for entry in manifest['sourceArchiveEntries']:
                self.assertEqual(generator.sha(bundle.read(entry['path'])), entry['sha256'])


if __name__ == '__main__':
    unittest.main()
