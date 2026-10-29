"""Synthetic certificate records, never proof of valid Authenticode signing."""
import struct
import unittest

from review_nsis_payload import payloads
from test_review_nsis_payload import envelope

def with_optional_header(raw):
    body=bytearray(raw);pe=128;optional=152
    struct.pack_into('<H',body,pe+6,1)
    struct.pack_into('<H',body,pe+20,224)
    struct.pack_into('<H',body,optional,0x10b)
    struct.pack_into('<I',body,optional+60,512)
    struct.pack_into('<I',body,optional+92,16)
    return body

def attach_record(raw=None):
    body=with_optional_header(envelope() if raw is None else raw)
    offset=(len(body)+7)&~7
    record=struct.pack('<IHH',16,0x200,2)+b'opaque!!'
    struct.pack_into('<II',body,152+96+32,offset,len(record))
    return bytes(body)+bytes(offset-len(body))+record

class CertificateEnvelopeTests(unittest.TestCase):
    def test_opt_in_inspects_payload_but_never_trusts_certificate(self):
        value=payloads(attach_record(),allow_certificate_envelope=True)
        self.assertEqual(value['extractions'],payloads(envelope())['extractions'])
        self.assertFalse(value['certificateEnvelope']['signatureVerified'])
        self.assertFalse(value['certificateEnvelope']['publisherVerified'])
    def test_default_still_rejects_certificate_append(self):
        with self.assertRaisesRegex(ValueError,'appended envelope'):payloads(attach_record())
    def test_unsigned_real_header_remains_supported_in_opt_in(self):
        value=payloads(bytes(with_optional_header(envelope())),allow_certificate_envelope=True)
        self.assertFalse(value['certificateEnvelope']['present'])
    def test_pe32_plus_certificate_directory_offsets(self):
        blob=bytearray(attach_record());offset,size=struct.unpack_from('<II',blob,280)
        struct.pack_into('<H',blob,148,240)
        struct.pack_into('<H',blob,152,0x20b)
        struct.pack_into('<I',blob,152+108,16)
        struct.pack_into('<II',blob,280,0,0)
        struct.pack_into('<II',blob,296,offset,size)
        value=payloads(bytes(blob),allow_certificate_envelope=True)
        self.assertTrue(value['certificateEnvelope']['present'])
    def test_unknown_overlay_without_table_still_rejected(self):
        with self.assertRaisesRegex(ValueError,'appended envelope'):
            payloads(bytes(with_optional_header(envelope()))+b'junk',allow_certificate_envelope=True)
    def test_nonterminal_or_outside_table_rejected(self):
        for blob in (attach_record()+b'junk',attach_record()[:-1]):
            with self.subTest(),self.assertRaisesRegex(ValueError,'terminal'):
                payloads(blob,allow_certificate_envelope=True)
    def test_wrong_certificate_type_revision_and_length_rejected(self):
        for field,value in ((0,0),(4,0x100),(6,1)):
            blob=bytearray(attach_record());offset=struct.unpack_from('<I',blob,280)[0]
            struct.pack_into('<I' if field==0 else '<H',blob,offset+field,value)
            with self.subTest(field=field),self.assertRaises(ValueError):payloads(bytes(blob),allow_certificate_envelope=True)
    def test_crc_remains_required_under_certificate_envelope(self):
        blob=bytearray(attach_record());blob[550]^=1
        with self.assertRaisesRegex(ValueError,'CRC mismatch'):payloads(bytes(blob),allow_certificate_envelope=True)
    def test_nonzero_pre_certificate_padding_rejected(self):
        blob=bytearray(attach_record());end=len(envelope());offset=struct.unpack_from('<I',blob,280)[0]
        self.assertGreater(offset,end)
        blob[end]=1
        with self.assertRaisesRegex(ValueError,'between NSIS'):payloads(bytes(blob),allow_certificate_envelope=True)
    def test_section_overlap_rejected(self):
        blob=bytearray(attach_record());offset=struct.unpack_from('<I',blob,280)[0]
        struct.pack_into('<II',blob,376+16,offset,512)
        with self.assertRaisesRegex(ValueError,'overlaps PE section'):payloads(bytes(blob),allow_certificate_envelope=True)
    def test_unreviewed_optional_header_layout_rejected(self):
        blob=bytearray(attach_record());struct.pack_into('<H',blob,152,0)
        with self.assertRaisesRegex(ValueError,'magic'):payloads(bytes(blob),allow_certificate_envelope=True)

if __name__=='__main__':unittest.main()
