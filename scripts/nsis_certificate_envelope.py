"""Bounded PE certificate-table envelope for NSIS payload inspection only.

This does not decode PKCS#7, validate Authenticode, or establish a publisher.
Only one revision-2 PKCS_SIGNED_DATA table at EOF is supported. Other layouts
must be reviewed separately. Structure: Microsoft PE Format, Certificate Table.
"""
import hashlib
import struct

MAX_CERTIFICATE_BYTES=1024*1024
NSIS_SIGNATURE=b'\xef\xbe\xad\xdeNullsoftInst'

def require(value,message):
    if not value:raise ValueError(message)

def split(installer):
    require(len(installer)>=512 and installer[:2]==b'MZ','Certificate envelope requires PE')
    pe=struct.unpack_from('<I',installer,60)[0]
    require(64<=pe and pe+24<=len(installer) and installer[pe:pe+4]==b'PE\0\0','Invalid PE header')
    sections=struct.unpack_from('<H',installer,pe+6)[0]
    optional_size=struct.unpack_from('<H',installer,pe+20)[0]
    optional=pe+24
    require(1<=sections<=96 and optional_size>=2 and optional+optional_size<=len(installer),'Invalid optional header/section count')
    magic=struct.unpack_from('<H',installer,optional)[0]
    require(magic in (0x10b,0x20b),'Unsupported optional-header magic')
    directories=96 if magic==0x10b else 112
    count_offset=92 if magic==0x10b else 108
    require(optional_size>=directories+40,'Certificate directory outside optional header')
    require(struct.unpack_from('<I',installer,optional+count_offset)[0]>=5,'Certificate directory not declared')
    certificate_field=optional+directories+32
    # NSIS CRC excludes the first 512 bytes. The supported envelope requires
    # the signer-modified checksum and certificate-directory fields there.
    require(certificate_field+8<=512 and optional+68<=512,'Signer-modified headers outside reviewed NSIS CRC exclusion')
    offset,size=struct.unpack_from('<II',installer,certificate_field)
    if offset==size==0:
        return installer,{'present':False,'signatureVerified':False,'publisherVerified':False}
    require(offset>0 and 8<size<=MAX_CERTIFICATE_BYTES and offset%8==0
            and offset+size==len(installer),'Certificate table must be bounded, aligned and terminal')
    section_table=optional+optional_size
    require(section_table+sections*40<=offset,'Truncated PE section table')
    headers=struct.unpack_from('<I',installer,optional+60)[0]
    require(section_table+sections*40<=headers<=offset,'Invalid PE headers size')
    for i in range(sections):
        raw_size,raw_offset=struct.unpack_from('<II',installer,section_table+i*40+16)
        require(raw_size==0 or (raw_offset>=headers and raw_offset+raw_size<=offset),'Certificate table overlaps PE section data')
    length,revision,kind=struct.unpack_from('<IHH',installer,offset)
    require(8<length<=size and (length+7)&~7==size,'Only one complete certificate record is supported')
    require(revision==0x0200 and kind==0x0002,'Unsupported WIN_CERTIFICATE revision/type')
    require(not any(installer[offset+length:]),'Nonzero certificate alignment padding')
    # Certificate content remains opaque and explicitly unverified. Checking
    # the record is not evidence that it contains a valid or trusted signature.
    prefix=installer[:offset]
    position=prefix.find(NSIS_SIGNATURE)
    require(position>=4 and prefix.find(NSIS_SIGNATURE,position+1)<0,'Unique NSIS header required before certificate table')
    nsis=position-4
    require(nsis>=512 and nsis%512==0 and nsis+28<=offset,'Invalid NSIS envelope position')
    following=struct.unpack_from('<I',installer,nsis+24)[0]
    end=nsis+following
    require(following>=37 and end<=offset and ((end+7)&~7)==offset,'Certificate is not immediately after the aligned NSIS envelope')
    require(not any(installer[end:offset]),'Nonzero bytes between NSIS and certificate table')
    metadata={'present':True,'certificateOffset':offset,'certificateTableBytes':size,
              'recordBytes':length,'revision':revision,'certificateType':kind,
              'certificateTableSha256':hashlib.sha256(installer[offset:]).hexdigest(),
              'nsisEnvelopeEnd':end,'alignmentPaddingBytes':offset-end,
              'signatureVerified':False,'publisherVerified':False,
              'limitation':'Certificate content is opaque; PKCS#7, image digest, signer chain, timestamp and revocation are not verified'}
    return installer[:end],metadata
