import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('candidate', ROOT / 'scripts/source-candidate.py')
candidate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(candidate)


class SourceCandidateTests(unittest.TestCase):
    def setUp(self):
        self.parent = ROOT / 'reports/source-candidate-test-temp'
        self.parent.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(dir=self.parent)).resolve()
        self.addCleanup(self.remove_owned)
        (self.root / 'scripts').mkdir()
        (self.root / 'src').mkdir()
        (self.root / 'reports/private').mkdir(parents=True)
        (self.root / 'reports/private/credential.xml').write_text('private fixture')
        (self.root / 'src/main.rs').write_text('fn main() {}\n', encoding='utf-8')
        self.policy = {'schema': 1, 'files': [candidate.POLICY], 'trees': {'src': ['.rs']}}
        self.save_policy()
        self.plan = self.root / 'reports/plan.json'
        self.output = self.root / 'reports/source-export-test'

    def remove_owned(self):
        # Verify the exact recursively deleted test directory first.
        resolved = self.root.resolve()
        if resolved.parent != self.parent.resolve() or not resolved.name.startswith('tmp'):
            raise ValueError('Test cleanup boundary changed')
        shutil.rmtree(resolved)

    def save_policy(self):
        (self.root / candidate.POLICY).write_text(json.dumps(self.policy), encoding='utf-8')

    def make_plan(self):
        candidate.write_new(self.plan, candidate.inventory(self.root))
        return candidate.digest(self.plan.read_bytes())

    def test_export_exact_sources_excludes_private_reports_and_preserves_bytes(self):
        result = candidate.export(self.root, self.plan, self.make_plan(), self.output)
        with zipfile.ZipFile(self.output / 'source.zip') as archive:
            self.assertEqual(set(archive.namelist()), {candidate.POLICY, 'src/main.rs',
                             'SOURCE-REVIEW.md', 'SOURCE-MANIFEST.json', 'reports/.gitkeep'})
            self.assertEqual(archive.read('src/main.rs'), (self.root / 'src/main.rs').read_bytes())
        self.assertFalse(result['publicationAuthorized'])
        self.assertFalse(result['projectLicenseGranted'])
        self.assertFalse(result['buildVerified'])
        self.assertTrue((self.root / 'reports/private/credential.xml').exists())

    def test_source_edit_after_plan_refuses_before_output(self):
        digest = self.make_plan()
        (self.root / 'src/main.rs').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'inventory changed'):
            candidate.export(self.root, self.plan, digest, self.output)
        self.assertFalse(self.output.exists())

    def test_new_file_after_plan_and_wrong_plan_digest_refused(self):
        digest = self.make_plan()
        with self.assertRaisesRegex(ValueError, 'digest differs'):
            candidate.export(self.root, self.plan, '0' * 64, self.output)
        (self.root / 'src/extra.rs').write_text('fn extra() {}')
        with self.assertRaisesRegex(ValueError, 'inventory changed'):
            candidate.export(self.root, self.plan, digest, self.output)

    def test_private_traversal_and_absolute_paths_cannot_enter_policy(self):
        for path in ('reports/private/credential.xml', '../outside.rs', '/outside.rs',
                     'C:/outside.rs', 'src/../outside.rs', 'src\\outside.rs', 'src/.env.local'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                candidate.safe_name(path)

    def test_credential_marker_is_refused_without_echoing_value(self):
        marker = 'sk-' + 'a' * 40
        (self.root / 'src/main.rs').write_text(marker)
        with self.assertRaises(ValueError) as error:
            candidate.inventory(self.root)
        self.assertIn('src/main.rs', str(error.exception))
        self.assertNotIn(marker, str(error.exception))

    def test_existing_or_external_output_never_replaced(self):
        digest = self.make_plan()
        self.output.mkdir()
        marker = self.output / 'retained.txt'; marker.write_text('keep')
        with self.assertRaisesRegex(ValueError, 'Fresh local'):
            candidate.export(self.root, self.plan, digest, self.output)
        self.assertEqual(marker.read_text(), 'keep')
        with self.assertRaisesRegex(ValueError, 'Fresh local'):
            candidate.export(self.root, self.plan, digest, self.root / 'source-export-outside')

    def test_reviewed_fixture_exception_binds_marker_and_whole_source(self):
        marker = ('sk-' + 'synthetic-marker-for-export-test').encode()
        body = b'const FIXTURE: &str = "' + marker + b'";\n'
        source = self.root / 'src/main.rs'; source.write_bytes(body)
        self.policy['fixtureMarkers'] = [{'path': 'src/main.rs',
            'sourceSha256': candidate.digest(body), 'markerSha256': candidate.digest(marker),
            'reason': 'Synthetic test placeholder'}]
        self.save_policy()
        candidate.inventory(self.root)
        source.write_bytes(body + b'// changed\n')
        with self.assertRaisesRegex(ValueError, 'credential marker'):
            candidate.inventory(self.root)

    def test_hardlinked_input_is_refused(self):
        os.link(self.root / 'src/main.rs', self.root / 'reports/alias.rs')
        with self.assertRaisesRegex(ValueError, 'Invalid source file'):
            candidate.inventory(self.root)

    def test_unreviewed_extension_and_case_alias_rejected(self):
        extra = self.root / 'src/other.dat'; extra.write_bytes(b'data')
        with self.assertRaisesRegex(ValueError, 'extension'):
            candidate.inventory(self.root)
        extra.unlink()
        self.policy['files'].append('src/MAIN.rs'); self.save_policy()
        with self.assertRaisesRegex(ValueError, 'Case alias'):
            candidate.inventory(self.root)


if __name__ == '__main__':
    unittest.main()
