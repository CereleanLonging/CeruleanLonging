"""Create/verify a sealed local evidence set. No recovery, execution or network.

The expected index/manifest digest must come from a separately retained receipt.
Integrity is not provider authenticity, release acceptance, or secret redaction.
"""
import argparse
from contextlib import ExitStack, contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat

MAX_FILES = 10000
MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
MAX_INDEX = 4 * 1024 * 1024
KIND = 'cerulean-portable-evidence-v1'
README = b'''CeruleanLonging portable evidence v1
Verify this folder using portable-evidence.py verify --archive <folder>
--manifest-sha256 <digest saved separately at creation>.
All payload paths are relative to payload/. The source workspace is unnecessary.
Verification reads bytes only. It does not execute bundled programs, resume tasks,
restore a workbench, attest provider reality, or approve a release.
The selected set may contain source code, logs, original paths and user content.
Known credential filenames are refused, but this is not content redaction.
Review the selected content before sharing; creation never uploads anything.
'''


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def digest(value):
    require(isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value), 'Expected lowercase SHA256 required')
    return value


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'Duplicate JSON field')
        result[key] = value
    return result


def parse(data):
    return json.loads(data, object_pairs_hook=object_pairs)


def encode(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def safe_name(name):
    require(isinstance(name, str) and 0 < len(name) <= 4096, 'Invalid evidence path')
    require(not any(c in name for c in '\\:\x00') and not any(ord(c) < 32 for c in name), 'Nonportable evidence path')
    parts = name.split('/')
    require(len(parts) <= 64, 'Evidence path nesting exceeds bound')
    require(all(p and p not in ('.', '..') and not p.endswith((' ', '.')) for p in parts), 'Traversal or aliased evidence path')
    require(not PurePosixPath(name).is_absolute(), 'Absolute payload path forbidden')
    for part in parts:
        lower = part.lower()
        require(not re.fullmatch(r'(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?', lower), 'Windows reserved path')
        require(lower not in {'credentials', '.aws', '.ssh', '.gnupg', 'auth.json', 'credentials.json', 'credentials.json.dpapi', 'id_rsa', 'id_ed25519'}
                and not lower.startswith('.env') and not lower.endswith(('.pem', '.pfx', '.p12', '.key', '.dpapi')), 'Known credential path refused')
    return parts


def inventory(value):
    require(isinstance(value, list) and 0 < len(value) <= MAX_FILES, 'Evidence inventory size invalid')
    seen, total = set(), 0
    for item in value:
        require(isinstance(item, dict) and set(item) == {'path', 'bytes', 'sha256'}, 'Unexpected evidence index fields')
        safe_name(item['path'])
        folded = item['path'].casefold()
        require(folded not in seen, 'Duplicate or case-aliased payload path')
        seen.add(folded)
        require(type(item['bytes']) is int and 0 <= item['bytes'] <= MAX_FILE, 'Evidence file exceeds bound')
        total += item['bytes']; digest(item['sha256'])
    require(total <= MAX_TOTAL, 'Evidence set exceeds bound')
    # No file may also be an ancestor of another selected file.
    for name in seen:
        require(not any(p.as_posix() in seen for p in PurePosixPath(name).parents if p != PurePosixPath('.')), 'File/directory collision')
    return total


def ordinary(path, directory):
    info = path.lstat()
    require(not stat.S_ISLNK(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400, 'Links/reparse points refused')
    require(stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode), 'Not an ordinary entry')
    if not directory:
        require(info.st_nlink == 1, 'Hard-linked evidence refused')
    return info


@contextmanager
def windows_pin(path, directory):
    """Deny directory deletion or file mutation while selected bytes are read."""
    if os.name != 'nt':
        yield
        return
    import ctypes
    from ctypes import wintypes as w
    k = ctypes.WinDLL('kernel32', use_last_error=True)
    k.CreateFileW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, w.LPVOID, w.DWORD, w.DWORD, w.HANDLE]
    k.CreateFileW.restype = w.HANDLE
    k.CloseHandle.argtypes = [w.HANDLE]
    flags = 0x00200000 | (0x02000000 if directory else 0)  # OPEN_REPARSE_POINT / BACKUP_SEMANTICS
    native = str(path)
    if not native.startswith('\\\\?\\'):
        native = '\\\\?\\UNC\\'+native[2:] if native.startswith('\\\\') else '\\\\?\\'+native
    handle = k.CreateFileW(native, 0x80000000, 3 if directory else 1, None, 3, flags, None)
    require(handle != ctypes.c_void_p(-1).value, 'Cannot pin selected evidence entry')
    try:
        ordinary(path, directory)
        yield
    finally:
        require(k.CloseHandle(handle), 'Cannot close evidence pin')


def absolute(path):
    path = Path(path)
    require(path.is_absolute() and '..' not in path.parts, 'Absolute path without parent traversal required')
    return path


def pin_directory(stack, directory):
    directory = absolute(directory)
    for parent in reversed((directory, *directory.parents)):
        stack.enter_context(windows_pin(parent, True)); ordinary(parent, True)
    return directory


def read_file(path, maximum):
    with ExitStack() as stack:
        pin_directory(stack, path.parent)
        stack.enter_context(windows_pin(path, False))
        before = ordinary(path, False)
        require(before.st_size <= maximum, 'Selected file exceeds read bound')
        with path.open('rb') as f:
            opened = os.fstat(f.fileno())
            require((before.st_dev, before.st_ino) == (opened.st_dev, opened.st_ino), 'Selected file identity changed')
            content = f.read(maximum + 1)
            after = os.fstat(f.fileno())
        require(len(content) <= maximum and (opened.st_size, opened.st_mtime_ns) == (after.st_size, after.st_mtime_ns), 'Selected file changed during read')
        return content


def write_new(path, content):
    with path.open('xb') as f:
        f.write(content); f.flush(); os.fsync(f.fileno())


def create(root, index, expected, destination):
    root, index, destination = map(absolute, (root, index, destination))
    digest(expected)
    with ExitStack() as pins:
        pin_directory(pins, root); pin_directory(pins, destination.parent)
        root = root.resolve(); destination = destination.parent.resolve()/destination.name
        # A new external directory prevents the archive from mutating the source set.
        require(not destination.is_relative_to(root) and not root.is_relative_to(destination), 'Archive must be outside the source root')
        require(not os.path.lexists(destination), 'Archive already exists; inspect original, do not overwrite/retry')
        raw = read_file(index, MAX_INDEX); require(sha(raw) == expected, 'Source index digest changed')
        files = parse(raw); total = inventory(files)
        manifest = dict(schema=1, kind=KIND, sourceIndexSha256=expected, files=files,
                        readmeSha256=sha(README), totalBytes=total, executionAllowed=False,
                        restorableWorkbench=False, realityAttested=False, releaseAccepted=False)
        manifest_bytes = encode(manifest); require(len(manifest_bytes) <= MAX_INDEX, 'Manifest exceeds bound')
        # Publish only after all selected bytes were copied and verified. Failed
        # archives keep their partial folder and have no final manifest.
        destination.mkdir(); pins.enter_context(windows_pin(destination, True))
        write_new(destination/'manifest.pending', manifest_bytes)
        payload = destination/'payload'; payload.mkdir(); write_new(destination/'README.txt', README)
        for item in files:
            content = read_file(root.joinpath(*safe_name(item['path'])), MAX_FILE)
            require(len(content) == item['bytes'] and sha(content) == item['sha256'], 'Selected evidence changed: '+item['path'])
            target = payload.joinpath(*safe_name(item['path']))
            target.parent.mkdir(parents=True, exist_ok=True)
            with ExitStack() as parents:
                pin_directory(parents, target.parent); write_new(target, content)
        # create-new hard-link publication refuses a competing final manifest.
        os.link(destination/'manifest.pending', destination/'manifest.json')
        (destination/'manifest.pending').unlink()
    return verify(destination, sha(manifest_bytes))


def verify(archive, expected):
    archive = absolute(archive); digest(expected)
    with ExitStack() as pins:
        pin_directory(pins, archive)
        import itertools
        require({p.name for p in itertools.islice(archive.iterdir(), 4)} == {'manifest.json', 'README.txt', 'payload'}, 'Incomplete archive or unexpected root entry')
        raw = read_file(archive/'manifest.json', MAX_INDEX)
        require(sha(raw) == expected, 'Archive manifest differs from separately retained digest')
        manifest = parse(raw)
        require(isinstance(manifest, dict) and set(manifest) == {'schema','kind','sourceIndexSha256','files','readmeSha256','totalBytes','executionAllowed','restorableWorkbench','realityAttested','releaseAccepted'}, 'Unexpected manifest fields')
        require(type(manifest['schema']) is int and manifest['schema'] == 1 and manifest['kind'] == KIND, 'Unsupported archive kind')
        for flag in ['executionAllowed','restorableWorkbench','realityAttested','releaseAccepted']:require(manifest[flag] is False, 'Archive cannot grant authority')
        digest(manifest['sourceIndexSha256'])
        total = inventory(manifest['files'])
        require(type(manifest['totalBytes']) is int and total == manifest['totalBytes'], 'Archive total differs')
        require(sha(read_file(archive/'README.txt', 8192)) == manifest['readmeSha256'] == sha(README), 'Archive usage note changed')
        payload = archive/'payload'; pin_directory(pins, payload)
        expected_files = {f['path'] for f in manifest['files']}
        expected_dirs = {p.as_posix() for f in expected_files for p in PurePosixPath(f).parents if p != PurePosixPath('.')}
        actual_files, actual_dirs = set(), set()
        def walk(parent, relative=''):
            for child in parent.iterdir():
                name = relative+'/'+child.name if relative else child.name
                safe_name(name)
                info = child.lstat()
                ordinary(child, stat.S_ISDIR(info.st_mode))
                if stat.S_ISDIR(info.st_mode):
                    require(name in expected_dirs, 'Unexpected archive directory')
                    ordinary(child, True); actual_dirs.add(name); walk(child, name)
                else:
                    require(name in expected_files, 'Unexpected archive file')
                    ordinary(child, False); actual_files.add(name)
        walk(payload)
        require(actual_files == expected_files and actual_dirs == expected_dirs, 'Archive inventory differs')
        for item in manifest['files']:
            content = read_file(payload.joinpath(*safe_name(item['path'])), MAX_FILE)
            require(len(content) == item['bytes'] and sha(content) == item['sha256'], 'Archive payload differs: '+item['path'])
        return dict(kind=KIND, manifestSha256=expected, sourceIndexSha256=manifest['sourceIndexSha256'],
                    filesVerified=len(expected_files), bytesVerified=total, sourceWorkspaceRequired=False,
                    credentialsContentRedacted=False, executionAllowed=False, restorableWorkbench=False,
                    realityAttested=False, releaseAccepted=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    creation = commands.add_parser('create')
    for name in ['root','index','index-sha256','to']:creation.add_argument('--'+name, required=True)
    verification = commands.add_parser('verify')
    verification.add_argument('--archive', required=True);verification.add_argument('--manifest-sha256', required=True)
    args = parser.parse_args()
    try:
        result = create(args.root,args.index,args.index_sha256,args.to) if args.command == 'create' else verify(args.archive,args.manifest_sha256)
        print(json.dumps(result, ensure_ascii=False, indent=2))
    except (OSError, ValueError) as error:
        parser.exit(1, 'Evidence archive refused: '+str(error)+'\n')


if __name__ == '__main__':main()
