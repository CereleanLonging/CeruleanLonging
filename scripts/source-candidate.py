"""Create a local, explicitly enumerated source review candidate; never publish.

No project license is granted. This is neither a secret-free certification nor
proof of a hosted build. Historical reports and local credentials are excluded.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import zipfile

ROOT = Path(__file__).resolve().parents[1]
POLICY = 'scripts/source-candidate-policy.json'
MAX_FILE = 32 * 1024 * 1024
MAX_TOTAL = 128 * 1024 * 1024
DENIED = {'reports', 'handoffs', 'node_modules', 'target', 'dist', '.git', '.local', '__pycache__', 'binaries', 'gen'}
SECRET = re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|(?:sk-|ghp_|github_pat_)[A-Za-z0-9_.-]{24,}')
REVIEW_NOTE = b'''# Local source review candidate

Private preparation only. No publication or license grant is authorized.
Review contributor rights, project license, contacts, all contents and hosted
build inputs before any public upload or SignPath application.

Source and existing CI inputs are copied byte-for-byte from a hash-bound plan.
No historical reports, credentials, user profiles or built binaries are included.
An empty reports directory is generated for tests that create their own outputs.
Explicitly ignored live/archived-fixture tests still require their documented
external environment; this export does not claim those tests are self-contained.

Node/Rust dependencies and NSIS tools must be installed and independently match
the reviewed byte identities. No dependencies are downloaded by this exporter.
Cargo metadata checks do not prove compilation or GitHub-hosted provenance.
No project LICENSE is fabricated. Third-party licenses keep their own terms.
'''


def need(value, message):
    if not value:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def safe_name(name):
    need(isinstance(name, str) and name and '\\' not in name and ':' not in name,
         'Unsafe candidate path')
    path = PurePosixPath(name)
    need(not path.is_absolute() and path.as_posix() == name
         and all(part not in ('.', '..', '') for part in name.split('/')), 'Unsafe candidate path')
    parts = [p.casefold() for p in path.parts]
    need(not any(p in DENIED or p.startswith('.env') for p in parts), 'Private/generated path excluded')
    need(path.suffix.casefold() not in ('.exe', '.dll', '.pfx', '.p12', '.pem', '.key', '.xml'),
         'Binary or credential path excluded')
    return path


def ordinary(path):
    for part in (path, *path.parents):
        info = part.lstat()
        need(not stat.S_ISLNK(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400,
             'Reparse path refused')


def read(root, name, fixture_markers=()):
    safe_name(name)
    path = root / name
    ordinary(path)
    info = path.stat()
    need(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_size <= MAX_FILE,
         'Invalid source file: ' + name)
    with path.open('rb') as f:
        data = f.read(MAX_FILE + 1)
    need(len(data) <= MAX_FILE, 'Source exceeds limit')
    if path.suffix.casefold() not in ('.zip', '.ico', '.png'):
        approved = {(r['path'], r['sourceSha256'], r['markerSha256']) for r in fixture_markers}
        for match in SECRET.finditer(data):
            # Only exact, reviewed fake-key literals in unchanged test sources.
            need(re.fullmatch(rb'sk-[a-z]+(?:-[a-z0-9]+)+', match.group()) is not None
                 and (name, digest(data), digest(match.group())) in approved,
                 'Potential credential marker; review locally: ' + name)
    return data


def select(root, policy):
    need(policy.get('schema') == 1 and {'schema', 'files', 'trees'} <= set(policy)
         and set(policy) <= {'schema', 'files', 'trees', 'fixtureMarkers'}, 'Unsupported policy')
    names = list(policy['files'])
    for prefix, extensions in policy['trees'].items():
        safe_name(prefix)
        base = root / prefix
        ordinary(base)
        need(base.is_dir(), 'Missing source tree: ' + prefix)
        for folder, dirs, files in os.walk(base, followlinks=False):
            for name in dirs:
                ordinary(Path(folder) / name)
                safe_name((Path(folder) / name).relative_to(root).as_posix())
            for name in files:
                path = Path(folder) / name
                relative = path.relative_to(root).as_posix()
                safe_name(relative)
                need(path.suffix in extensions, 'Unreviewed source extension: ' + relative)
                names.append(relative)
    # A fixed file may also be inside an approved tree, but case aliases may not.
    unique = sorted(set(names))
    need(len({n.casefold() for n in unique}) == len(unique), 'Case alias in source set')
    return unique


def inventory(root):
    policy_raw = read(root, POLICY)
    policy = json.loads(policy_raw)
    names = select(root, policy)
    rows = []
    total = 0
    for name in names:
        data = read(root, name, policy.get('fixtureMarkers', ()))
        total += len(data)
        need(total <= MAX_TOTAL, 'Candidate source aggregate bound')
        rows.append({'path': name, 'bytes': len(data), 'sha256': digest(data)})
    return {'schema': 1, 'policySha256': digest(policy_raw), 'files': rows,
            'totalBytes': total, 'publicationAuthorized': False, 'projectLicenseGranted': False,
            'credentialsCertifiedAbsent': False, 'buildVerified': False}


def write_new(path, value):
    with path.open('x', encoding='utf-8') as f:
        json.dump(value, f, ensure_ascii=False, indent=2)


def export(root, plan_path, expected_digest, output):
    raw = plan_path.read_bytes()
    need(digest(raw) == expected_digest, 'Reviewed plan digest differs')
    plan = json.loads(raw)
    need(plan == inventory(root), 'Source inventory changed since plan review')
    ordinary(output.parent)
    need(not output.exists() and output.parent == root / 'reports'
         and output.name.startswith('source-export-'), 'Fresh local reports/source-export-* directory required')
    fixture_markers = json.loads(read(root, POLICY)).get('fixtureMarkers', ())
    bodies = {}
    for row in plan['files']:
        body = read(root, row['path'], fixture_markers)
        need(len(body) == row['bytes'] and digest(body) == row['sha256'], 'Source changed during export')
        bodies[row['path']] = body
    # Keep generated review metadata distinguishable from original source bytes.
    bodies['SOURCE-REVIEW.md'] = REVIEW_NOTE
    bodies['reports/.gitkeep'] = b''
    bodies['SOURCE-MANIFEST.json'] = raw
    output.mkdir()
    tree = output / 'source'
    tree.mkdir()
    partial = output / 'source.zip.partial'
    with zipfile.ZipFile(partial, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for name, body in sorted(bodies.items()):
            target = tree / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as f:
                f.write(body)
            info = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, body, compresslevel=9)
    with zipfile.ZipFile(partial) as z:
        need(len(z.infolist()) == len(bodies) and set(z.namelist()) == set(bodies), 'Archive file set differs')
        for name, body in bodies.items():
            need(z.read(name) == body and (tree / name).read_bytes() == body, 'Export bytes differ')
    partial.rename(output / 'source.zip')
    result = {'status': 'local-source-review-candidate', 'sourceFiles': len(plan['files']),
              'generatedFiles': ['SOURCE-REVIEW.md', 'SOURCE-MANIFEST.json', 'reports/.gitkeep'],
              'planSha256': expected_digest, 'archiveSha256': digest((output / 'source.zip').read_bytes()),
              'publicationAuthorized': False, 'projectLicenseGranted': False, 'buildVerified': False}
    write_new(output / 'result.json', result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    p = sub.add_parser('plan'); p.add_argument('--output', type=Path, required=True)
    p = sub.add_parser('export'); p.add_argument('--plan', type=Path, required=True)
    p.add_argument('--plan-sha256', required=True); p.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.action == 'plan':
        value = inventory(ROOT); write_new(args.output, value)
        print(json.dumps({'sourceFiles': len(value['files']), 'totalBytes': value['totalBytes'],
                          'planSha256': digest(args.output.read_bytes())}))
    else:
        print(json.dumps(export(ROOT, args.plan, args.plan_sha256, args.output.absolute())))


if __name__ == '__main__':
    main()
