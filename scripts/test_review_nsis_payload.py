"""Bounded, entirely in-memory malformed archive checks; no installation."""
import copy
import json
import lzma
from pathlib import Path
import struct
import tempfile
import unittest
import zlib

from review_nsis_payload import SIGNATURE, RESOURCE_LIMITS, payloads, review, sha


GUI = b"MZ-gui-fixture"
CORE = b"MZ-core-fixture"
NOTICE = b"SQLite component license fixture\n"


def fixture_records(offset_change=0, duplicate=False, embedded_decoy=False, notice=None, resources=None):
    names = ["CeruleanLonging.exe", "cerulean-runtime.exe"]
    blobs = [GUI, CORE]
    if notice is not None:
        names.append("SQLITE-NOTICES.txt")
        blobs.append(notice)
    if resources:
        names.extend(resources.keys())
        blobs.extend(resources.values())
    if embedded_decoy:
        blobs[0] = b"prefix"+GUI
    if duplicate:
        names.append(names[0])
        blobs.append(GUI)
    string_indexes, strings = [], b""
    for name in names:
        string_indexes.append(len(strings)//2)
        strings += name.encode("utf-16-le")+b"\0\0"
    table_end = 300+28*len(names)
    header = bytearray(table_end+len(strings))
    struct.pack_into("<II", header, 4+2*8, 300, len(names))
    struct.pack_into("<II", header, 4+3*8, table_end, 0)
    struct.pack_into("<II", header, 4+4*8, len(header), 0)
    header[table_end:] = strings
    blocks = b""
    for index, blob in enumerate(blobs):
        struct.pack_into("<7i", header, 300+28*index, 20, 0, string_indexes[index],
                         len(blocks)+(offset_change if index == 0 else 0), 0, 0, 0)
        blocks += struct.pack("<I", len(blob))+blob
    return struct.pack("<I", len(header))+header+blocks


def envelope(decoded=None, properties=93, dictionary=8388608):
    decoded = fixture_records() if decoded is None else decoded
    compressed = lzma.compress(decoded, format=lzma.FORMAT_RAW, filters=[
        dict(id=lzma.FILTER_LZMA1, dict_size=8388608, lc=3, lp=0, pb=2)])
    prefix = bytearray(512)
    prefix[:2] = b"MZ"
    struct.pack_into("<I", prefix, 60, 128)
    prefix[128:132] = b"PE\0\0"
    payload = bytes([properties])+struct.pack("<I", dictionary)+compressed
    header = struct.pack("<I", 0)+SIGNATURE+struct.pack(
        "<II", struct.unpack_from("<I", decoded)[0], 28+len(payload)+4)
    body = bytes(prefix)+header+payload
    return body+struct.pack("<I", zlib.crc32(body[512:]))


def package_review(installer, mutate_manifest=None, requested_digest=None, notice=None, resources=None):
    with tempfile.TemporaryDirectory(prefix="cerulean-payload-test-") as directory:
        root = Path(directory)
        files = {"setup.exe": installer, "CeruleanLonging.exe": GUI, "cerulean-runtime.exe": CORE}
        manifest = dict(schema=1, product="CeruleanLonging", version="test",
                        artifacts=[dict(path=n, bytes=len(b), sha256=sha(b)) for n,b in files.items()])
        if notice is not None:
            files['SQLITE-NOTICES.txt'] = notice
            manifest['version'] = '0.1.21'
            manifest['resources'] = [dict(path='SQLITE-NOTICES.txt',bytes=len(notice),sha256=sha(notice))]
        if resources is not None:
            files.update(resources)
            manifest['version'] = '0.1.22'
            manifest['resources'] = [dict(path=name, bytes=len(body), sha256=sha(body)) for name, body in resources.items()]
        if mutate_manifest:
            mutate_manifest(manifest)
        raw = json.dumps(manifest).encode()
        (root/"manifest.json").write_bytes(raw)
        for name, content in files.items():
            (root/name).write_bytes(content)
        return review(root, requested_digest or sha(raw))


class PayloadTests(unittest.TestCase):
    def test_full_notice_resources_must_match_actual_installer_records(self):
        resources = {name: ('fixture for ' + name).encode() for name in RESOURCE_LIMITS}
        result = package_review(envelope(fixture_records(resources=resources)), resources=resources)
        self.assertEqual(len(result['verified']), 7)
        self.assertEqual({r['name'] for r in result['verified'][2:]}, set(resources))

    def test_full_notice_missing_duplicate_or_traversal_is_rejected(self):
        resources = {name: name.encode() for name in RESOURCE_LIMITS}
        def remove(m):
            m['resources'].pop()
        def duplicate(m):
            m['resources'][-1] = m['resources'][0]
        def traversal(m):
            m['resources'][-1]['path'] = '../THIRD-PARTY-MANIFEST.json'
        for mutate in (remove, duplicate, traversal):
            with self.subTest(mutation=mutate.__name__), self.assertRaisesRegex(ValueError, 'notice resource missing or unexpected'):
                package_review(envelope(fixture_records(resources=resources)), mutate, resources=resources)

    def test_full_notice_external_copy_does_not_prove_installer_contains_it(self):
        resources = {name: name.encode() for name in RESOURCE_LIMITS}
        fewer = {n:b for n,b in resources.items() if n != 'THIRD-PARTY-SOURCES.zip'}
        with self.assertRaisesRegex(ValueError, 'extraction record absent'):
            package_review(envelope(fixture_records(resources=fewer)), resources=resources)
        altered = dict(resources, **{'THIRD-PARTY-NOTICES.txt': b'old notices'})
        with self.assertRaisesRegex(ValueError, 'payload differs'):
            package_review(envelope(fixture_records(resources=altered)), resources=resources)

    def test_required_notice_is_bound_to_exact_installer_record(self):
        result = package_review(envelope(fixture_records(notice=NOTICE)), notice=NOTICE)
        self.assertEqual(result['verified'][-1]['sha256'], sha(NOTICE))

    def test_new_version_without_notice_manifest_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'notice resource missing'):
            package_review(envelope(), lambda m: m.update(version='0.1.21'))

    def test_notice_manifest_cannot_name_an_external_path(self):
        def mutate(m):
            m['resources'][0]['path'] = '../SQLITE-NOTICES.txt'
        with self.assertRaisesRegex(ValueError, 'notice resource missing or unexpected'):
            package_review(envelope(fixture_records(notice=NOTICE)), mutate, notice=NOTICE)

    def test_notice_integrity_and_installer_payload_are_both_required(self):
        def mutate(m):
            m['resources'][0]['sha256'] = '0'*64
        with self.assertRaisesRegex(ValueError, 'notice integrity mismatch'):
            package_review(envelope(fixture_records(notice=NOTICE)), mutate, notice=NOTICE)
        with self.assertRaisesRegex(ValueError, 'payload differs'):
            package_review(envelope(fixture_records(notice=b'wrong')), notice=NOTICE)

    def test_missing_notice_extraction_cannot_pass_with_external_copy(self):
        with self.assertRaisesRegex(ValueError, 'extraction record absent'):
            package_review(envelope(), notice=NOTICE)

    def test_named_payload_binding(self):
        result = package_review(envelope())
        self.assertEqual([entry["sha256"] for entry in result["verified"]], [sha(GUI), sha(CORE)])
        self.assertFalse(result["installedFilesVerified"])
        self.assertFalse(result["releaseAccepted"])

    def test_offset_inside_blob_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "payload boundary"):
            payloads(envelope(fixture_records(offset_change=1)))

    def test_embedded_matching_bytes_do_not_count_as_matching_file(self):
        with self.assertRaisesRegex(ValueError, "payload differs"):
            package_review(envelope(fixture_records(embedded_decoy=True)))

    def test_duplicate_named_extraction_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            package_review(envelope(fixture_records(duplicate=True)))

    def test_wrong_manifest_digest_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "digest changed"):
            package_review(envelope(), requested_digest="0"*64)

    def test_stale_artifact_digest_is_rejected(self):
        def mutate(manifest):
            manifest["artifacts"][1]["sha256"] = "0"*64
        with self.assertRaisesRegex(ValueError, "artifact bytes differ"):
            package_review(envelope(), mutate)

    def test_duplicate_manifest_entry_is_rejected(self):
        def mutate(manifest):
            manifest["artifacts"][1] = copy.deepcopy(manifest["artifacts"][0])
        with self.assertRaisesRegex(ValueError, "artifact set"):
            package_review(envelope(), mutate)

    def test_crc_corruption_is_rejected(self):
        blob = bytearray(envelope())
        blob[-10] ^= 1
        with self.assertRaisesRegex(ValueError, "CRC mismatch"):
            payloads(bytes(blob))

    def test_unreviewed_overlay_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "appended envelope"):
            payloads(envelope()+b"unreviewed")

    def test_truncated_compression_is_rejected(self):
        blob = bytearray(envelope()[:-15])
        struct.pack_into("<I", blob, 512+24, len(blob)-512)
        struct.pack_into("<I", blob, len(blob)-4, zlib.crc32(blob[512:-4]))
        with self.assertRaisesRegex(ValueError, "LZMA stream"):
            payloads(bytes(blob))

    def test_expansion_limit(self):
        with self.assertRaisesRegex(ValueError, "Decoded payload exceeds"):
            payloads(envelope(), max_decoded=16)

    def test_dictionary_limit(self):
        with self.assertRaisesRegex(ValueError, "LZMA properties"):
            payloads(envelope(dictionary=2**30))

    def test_invalid_entry_table(self):
        decoded = bytearray(fixture_records())
        struct.pack_into("<I", decoded, 4+4+2*8+4, 900000)
        with self.assertRaisesRegex(ValueError, "Entry table size"):
            payloads(envelope(decoded))

    def test_invalid_block_length(self):
        decoded = fixture_records()+struct.pack("<I", 9999)+b"x"
        with self.assertRaisesRegex(ValueError, "block exceeds"):
            payloads(envelope(decoded))


if __name__ == "__main__":
    unittest.main()
