"""Read-only payload audit for the project's NSIS Unicode/solid-LZMA profile.

No installer or payload is executed. This verifies stored EXTRACTFILE records,
not control-flow reachability, installed files, Authenticode, or GUI behavior.
Unknown envelopes fail closed; this is deliberately not a general NSIS extractor.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import re
from pathlib import Path
import struct
from datetime import datetime, timezone
import zlib

from isolated_nsis_install import no_reparse, save_new
from nsis_certificate_envelope import split as split_certificate_envelope

SIGNATURE = b"\xef\xbe\xad\xdeNullsoftInst"
REQUIRED = ("setup.exe", "CeruleanLonging.exe", "cerulean-runtime.exe")
MAX_FILE = 128 * 1024 * 1024
MAX_DECODED = 128 * 1024 * 1024
MAX_DICTIONARY = 32 * 1024 * 1024
RESOURCE_LIMITS = {"SQLITE-NOTICES.txt": 1024 * 1024,
                   "THIRD-PARTY-NOTICES.txt": 8 * 1024 * 1024,
                   "RUST-LIBRARY-COPYRIGHT.html": 4 * 1024 * 1024,
                   "THIRD-PARTY-SOURCES.zip": 16 * 1024 * 1024,
                   "THIRD-PARTY-MANIFEST.json": 1024 * 1024}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_bounded(path, limit):
    no_reparse(path)
    with Path(path).open("rb") as stream:
        data = stream.read(limit + 1)
    require(len(data) <= limit, "Input exceeds size bound")
    return data


def payloads(installer, max_decoded=MAX_DECODED, *, allow_certificate_envelope=False):
    require(len(installer) <= MAX_FILE and installer[:2] == b"MZ", "Unsupported PE envelope")
    certificate_envelope = None
    if allow_certificate_envelope:
        installer, certificate_envelope = split_certificate_envelope(installer)
    require(len(installer) >= 64, "Truncated DOS header")
    pe = struct.unpack_from("<I", installer, 60)[0]
    require(pe + 24 <= len(installer) and installer[pe:pe+4] == b"PE\0\0", "Missing PE header")
    matches = []
    start = 0
    while True:
        at = installer.find(SIGNATURE, start)
        if at < 0:
            break
        matches.append(at - 4)
        require(len(matches) <= 1, "Ambiguous NSIS signature")
        start = at + len(SIGNATURE)
    require(len(matches) == 1, "Missing NSIS header")
    offset = matches[0]
    require(offset >= 512 and offset % 512 == 0 and offset + 33 < len(installer),
            "Unsupported NSIS header position")
    flags, _, _, _, _, header_size, following_size = struct.unpack_from("<7I", installer, offset)
    require(flags == 0, "Unsupported NSIS flags; only reviewed installer profile allowed")
    require(following_size >= 37 and offset + following_size == len(installer),
            "Truncated or appended envelope; review signed/other profiles separately")
    require(300 <= header_size <= 4 * 1024 * 1024, "Header size outside reviewed profile")
    expected_crc = struct.unpack_from("<I", installer, len(installer) - 4)[0]
    require(zlib.crc32(installer[512:-4]) == expected_crc, "NSIS CRC mismatch")
    properties = installer[offset + 28]
    dictionary = struct.unpack_from("<I", installer, offset + 29)[0]
    require(properties < 225 and 4096 <= dictionary <= MAX_DICTIONARY,
            "Unsupported or excessive LZMA properties")
    lc, lp, pb = properties % 9, properties // 9 % 5, properties // 45
    require(lc + lp <= 4, "Unsupported LZMA literal context")
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=[
        dict(id=lzma.FILTER_LZMA1, dict_size=dictionary, lc=lc, lp=lp, pb=pb)
    ])
    decoded = decoder.decompress(installer[offset+33:-4], max_length=max_decoded+1)
    require(len(decoded) <= max_decoded, "Decoded payload exceeds size bound")
    require(decoder.eof and not decoder.unused_data, "Truncated or trailing LZMA stream")
    require(len(decoded) >= 4 and struct.unpack_from("<I", decoded)[0] == header_size,
            "Decoded header length differs")
    blocks, position = {}, 0
    while position < len(decoded):
        require(position + 4 <= len(decoded), "Truncated block length")
        length = struct.unpack_from("<I", decoded, position)[0]
        end = position + 4 + length
        require(end <= len(decoded), "Payload block exceeds decoded stream")
        require(len(blocks) < 4096, "Too many payload blocks")
        blocks[position] = dict(offset=position, bytes=length,
                                sha256=sha(decoded[position+4:end]))
        position = end
    require(0 in blocks and blocks[0]["bytes"] == header_size, "Missing header block")
    header = decoded[4:4+header_size]
    tables = [struct.unpack_from("<II", header, 4+i*8) for i in range(8)]
    entry_offset, entry_count = tables[2]
    string_offset = tables[3][0]
    string_end = tables[4][0]
    require(300 <= entry_offset <= string_offset < string_end <= len(header),
            "Unsupported header tables")
    require(entry_count <= 100000 and entry_offset + entry_count*28 == string_offset,
            "Entry table size differs from reviewed Unicode profile")
    data_base = 4 + header_size
    extracts = []
    for index in range(entry_count):
        opcode, *arguments = struct.unpack_from("<7i", header, entry_offset+index*28)
        if opcode != 20:  # NSIS 3 Unicode EW_EXTRACTFILE, fixed reviewed compiler profile.
            continue
        string_index, file_offset = arguments[1:3]
        location = string_offset + string_index*2
        require(string_index >= 0 and string_offset <= location < string_end,
                "Extraction filename outside string table")
        end = location
        while end+2 <= string_end and header[end:end+2] != b"\0\0":
            end += 2
        require(end+2 <= string_end, "Unterminated extraction filename")
        name = header[location:end].decode("utf-16-le", errors="strict")
        absolute = data_base + file_offset
        require(file_offset >= 0 and absolute in blocks, "Extraction offset is not a payload boundary")
        extracts.append(dict(entry=index, name=name, dataOffset=file_offset, **blocks[absolute]))
    result = dict(nsisHeaderOffset=offset, crc32=f"{expected_crc:08x}", headerBytes=header_size,
                decodedBytes=len(decoded), dictionaryBytes=dictionary,
                blockCount=len(blocks), entryCount=entry_count, extractions=extracts,
                blocks=list(blocks.values()))
    if certificate_envelope is not None:
        result['certificateEnvelope'] = certificate_envelope
    return result


def review(package, manifest_digest, *, allow_certificate_envelope=False):
    package = Path(package).absolute()
    raw = read_bounded(package/"manifest.json", 1024*1024)
    require(sha(raw) == manifest_digest.lower(), "Reviewed manifest digest changed")
    manifest = json.loads(raw)
    require(manifest.get("schema") == 1 and manifest.get("product") == "CeruleanLonging",
            "Unexpected package manifest")
    artifacts = manifest["artifacts"]
    require(len(artifacts) == 3 and {a["path"] for a in artifacts} == set(REQUIRED),
            "Unexpected or duplicate artifact set")
    data = {}
    for item in artifacts:
        content = read_bounded(package/item["path"], MAX_FILE)
        require(len(content) == item["bytes"] and sha(content) == item["sha256"],
                "Manifest artifact bytes differ: " + item["path"])
        data[item["path"]] = content
    archive = payloads(data["setup.exe"], allow_certificate_envelope=allow_certificate_envelope)
    resources = manifest.get("resources", [])
    version = manifest.get("version", "")
    parsed_version = tuple(map(int, version.split('.'))) if re.fullmatch(r"\d+\.\d+\.\d+", version) else (0, 0, 0)
    requires_notice = parsed_version >= (0, 1, 21)
    if requires_notice or resources:
        expected = set(RESOURCE_LIMITS) if parsed_version >= (0, 1, 22) else {"SQLITE-NOTICES.txt"}
        require(len(resources) == len(expected) and {r['path'] for r in resources} == expected,
                "Required notice resource missing or unexpected")
        for entry in resources:
            content = read_bounded(package/entry["path"], RESOURCE_LIMITS[entry['path']])
            require(len(content) == entry["bytes"] and sha(content) == entry["sha256"], "SQLite notice integrity mismatch")
            data[entry["path"]] = content
    verified = []
    for name in [*REQUIRED[1:], *(entry["path"] for entry in resources)]:
        matches = [entry for entry in archive["extractions"] if entry["name"].casefold() == name.casefold()]
        require(len(matches) == 1 and matches[0]["name"] == name,
                "Required extraction record absent, ambiguous, or differently cased: " + name)
        record = matches[0]
        require(record["bytes"] == len(data[name]) and record["sha256"] == sha(data[name]),
                "Named extraction payload differs from frozen artifact: " + name)
        verified.append(record)
    return dict(schema=1, at=datetime.now(timezone.utc).isoformat(), status="passed",
                package=str(package), version=manifest["version"], manifestSha256=sha(raw),
                installerSha256=sha(data["setup.exe"]), archive=archive, verified=verified,
                profile=("NSIS3 Unicode solid LZMA, bounded certificate-table envelope; signature unverified"
                         if archive.get('certificateEnvelope', {}).get('present')
                         else "NSIS3 Unicode solid LZMA, unsigned unappended envelope"),
                installerExecuted=False, registryChanged=False, productLaunched=False,
                installedFilesVerified=False, controlFlowVerified=False,
                publisherVerified=False, releaseAccepted=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-certificate-envelope", action="store_true",
                        help="Inspect a bounded terminal certificate table; does NOT verify signatures or publisher")
    args = parser.parse_args()
    require(not args.output.exists(), "Preserve existing audit output")
    result = review(args.package, args.manifest_sha256,
                    allow_certificate_envelope=args.allow_certificate_envelope)
    save_new(args.output, result)
    print(json.dumps(dict(status=result["status"], version=result["version"],
                          verified=[r["name"] for r in result["verified"]],
                          output=str(args.output.absolute()), releaseAccepted=False)))


if __name__ == "__main__":
    main()
