"""Build/check offline redistribution materials from reviewed, hash-bound inputs."""
from pathlib import Path, PurePosixPath
import argparse
import hashlib
import io
import importlib.util
import json
import stat
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
FOLDER = ROOT / 'third-party'
OUTPUTS = ('THIRD-PARTY-NOTICES.txt', 'RUST-LIBRARY-COPYRIGHT.html',
           'THIRD-PARTY-SOURCES.zip', 'THIRD-PARTY-MANIFEST.json')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode('utf-8')


def dependency_fingerprint(root):
    # Ignore only product version bumps; dependency/configuration changes require
    # a new reviewed source inventory, including changes to features or targets.
    cargo = tomllib.loads((root / 'Cargo.lock').read_text('utf-8'))
    for package in cargo['package']:
        if package['name'].startswith('cerulean-'):
            package['version'] = '<product-version>'
    npm = json.loads((root / 'package-lock.json').read_text('utf-8'))
    npm['version'] = '<product-version>'
    npm['packages']['']['version'] = '<product-version>'
    manifests = {}
    for name in ('Cargo.toml', 'crates/core/Cargo.toml', 'crates/adapters/Cargo.toml',
                 'crates/domain/Cargo.toml', 'apps/desktop/src-tauri/Cargo.toml'):
        value = tomllib.loads((root / name).read_text('utf-8'))
        if 'package' in value:
            value['package']['version'] = '<product-version>'
        if 'workspace' in value and 'package' in value['workspace']:
            value['workspace']['package']['version'] = '<product-version>'
        manifests[name] = value
    return sha(canonical({'cargo': cargo, 'npm': npm, 'manifests': manifests}))


def zip_bytes(files):
    result = io.BytesIO()
    with zipfile.ZipFile(result, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for name, body in sorted(files.items()):
            info = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o444) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(info, body, compresslevel=9)
    return result.getvalue()


def generated(root=ROOT):
    folder = root / 'third-party'
    meta = json.loads((folder / 'distribution-inputs.json').read_text('utf-8'))
    assert meta['schema'] == 1
    assert dependency_fingerprint(root) == meta['dependencyFingerprint'], 'Dependency inventory is stale'
    raw = (folder / 'distribution-inputs.zip').read_bytes()
    assert len(raw) <= 32 * 1024 * 1024 and sha(raw) == meta['archiveSha256'], 'Notice inputs changed'
    files = {}
    with zipfile.ZipFile(io.BytesIO(raw)) as bundle:
        infos = bundle.infolist()
        expected = {item['path']: item for item in meta['files']}
        assert len(expected) == len(infos) == len({i.filename for i in infos}), 'Duplicate or missing source material'
        assert sum(i.file_size for i in infos) <= 64 * 1024 * 1024
        for info in infos:
            name = info.filename
            assert name in expected and not info.is_dir()
            assert not PurePosixPath(name).is_absolute() and '..' not in PurePosixPath(name).parts
            assert '\\' not in name and ':' not in name and info.file_size <= 16 * 1024 * 1024
            data = bundle.read(info)
            assert len(data) == expected[name]['bytes'] and sha(data) == expected[name]['sha256'], name
            files[name] = data

    text = [b'CeruleanLonging - third-party notices\n\n',
            b'These notices describe the components inventoried for this Windows distribution.\n',
            b'The inventory conservatively includes build-time dependencies; it is not an exact link map.\n',
            b'Upstream text is reproduced in UTF-8 below. OR/AND license expressions are retained.\n',
            b'Original notice bytes are preserved in THIRD-PARTY-SOURCES.zip under original-notices/.\n',
            b'No license for CeruleanLonging product code is granted by this file.\n\n',
            b'Rust library copyright texts are in RUST-LIBRARY-COPYRIGHT.html beside this file.\n',
            b'Unmodified MPL crate source archives and NSIS/plugin source are in THIRD-PARTY-SOURCES.zip.\n',
            b'Open SOURCE-ACCESS.txt within that ZIP for origins and source-access details.\n',
            b'THIRD-PARTY-MANIFEST.json describes versions, scope, material digests and known provenance limits.\n']
    encodings = meta.get('textEncodings', {})
    notice_names = {name for component in meta['components'] for name in component['notices']}
    assert set(encodings).issubset(notice_names), 'Encoding override does not identify a notice'
    assert all(value in ('utf-8', 'latin-1') for value in encodings.values()), 'Unreviewed text encoding'
    for component in meta['components']:
        text.append(('\n' + '=' * 72 + '\n' + component['name'] + ' ' + component['version'] +
                     '\nScope: ' + component['scope'] + '\nLicense: ' + component['license'] +
                     '\nSource: ' + component['origin'] + '\n').encode('utf-8'))
        for name in component['notices']:
            data = files[name]
            rendered = data.decode(encodings.get(name, 'utf-8')).encode('utf-8')
            text.extend([('\n--- ' + name + ' ---\n').encode('utf-8'), rendered, b'\n'])
    source_files = {name: files[name] for name in meta['sourceFiles']}
    source_files.update({'original-notices/' + name: files[name] for name in notice_names})
    access = ['Sources accompanying CeruleanLonging\n',
              'The original source archives below are supplied locally, without an additional charge.\n',
              'They retain their own licenses and notices. No CeruleanLonging product code is relicensed.\n',
              'MPL archives contain the unmodified source files used from the respective locked crates.\n',
              'The NSIS archive includes the CPL-1.0 LZMA module source and its build material.\n',
              'NSIS/plugin source is the upstream tagged source; upstream reproducible binaries are not attested here.\n',
              'Use an archive reader for .crate (gzip tar), .tar.gz and this ZIP.\n\n']
    for source in meta['sourceOrigins']:
        access.append(source['path'] + '\n' + source['url'] + '\nSHA256: ' + sha(files[source['path']]) + '\n\n')
    source_files['SOURCE-ACCESS.txt'] = ''.join(access).encode('utf-8')
    outputs = {OUTPUTS[0]: b''.join(text), OUTPUTS[1]: files[meta['rustLibraryCopyright']],
               OUTPUTS[2]: zip_bytes(source_files)}
    manifest = {key: meta[key] for key in ('schema', 'components', 'sourceOrigins', 'limitations')}
    manifest.update(dependencyFingerprint=meta['dependencyFingerprint'], textEncodings=encodings,
                    scope='Windows distribution inputs and conservative build dependency notices; not an exact SBOM',
                    files=[{'path': name, 'bytes': len(data), 'sha256': sha(data)} for name, data in outputs.items()],
                    sourceArchiveEntries=[{'path': name, 'bytes': len(data), 'sha256': sha(data)}
                                          for name, data in sorted(source_files.items())])
    outputs[OUTPUTS[3]] = (json.dumps(manifest, ensure_ascii=False, indent=2) + '\n').encode('utf-8')
    return outputs


def check_build_inputs(root=ROOT):
    spec = importlib.util.spec_from_file_location(
        'distribution_build_inputs', Path(__file__).with_name('distribution-build-inputs.py'))
    guard = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(guard)
    return guard.check(root)


if __name__ == '__main__':
    if not __debug__:
        raise SystemExit('Distribution verification requires Python assertions enabled')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    build_inputs = check_build_inputs() if args.check else None
    outputs = generated()
    if args.check:
        for name, body in outputs.items():
            assert (FOLDER / name).read_bytes() == body, 'Distribution material is stale: ' + name
    else:
        assert not any((FOLDER / name).exists() for name in OUTPUTS), 'Preserve prior reviewed material before regenerating'
        for name, body in outputs.items():
            with (FOLDER / name).open('xb') as stream:
                stream.write(body)
    print(json.dumps({'status': 'verified' if args.check else 'generated',
                      'buildInputs': build_inputs,
                      'resources': [{'path': name, 'bytes': len(body), 'sha256': sha(body)} for name, body in outputs.items()]}))
