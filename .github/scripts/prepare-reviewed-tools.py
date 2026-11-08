"""Provision the already reviewed packaging tools on a disposable GitHub runner."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
DOWNLOADS = {
    'rustup-init.exe': (
        'https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-pc-windows-msvc/rustup-init.exe',
        '6f4bef66261261fcb43131be8720bab817d403a09edec7455c371974b90bdb7e'),
    'nsis.zip': (
        'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.11/nsis-3.11.zip',
        'c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1'),
    'nsis_tauri_utils.dll': (
        'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll',
        '5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709'),
}


def matches(base, records):
    for item in records:
        path = base / item['path']
        if not path.is_file() or path.stat().st_size != item['bytes']:
            return False
        if hashlib.sha256(path.read_bytes()).hexdigest() != item['sha256']:
            return False
    return True


def download(name, directory):
    url, expected = DOWNLOADS[name]
    destination = directory / name
    subprocess.run(['curl.exe', '--fail', '--silent', '--show-error', '--location',
                    '--proto', '=https', '--proto-redir', '=https', '--max-time', '120',
                    '--url', url, '--output', str(destination)], check=True, timeout=130)
    if hashlib.sha256(destination.read_bytes()).hexdigest() != expected:
        raise RuntimeError('Reviewed download digest mismatch for ' + name)
    return destination


def provision():
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise RuntimeError('Tool provisioning is limited to disposable GitHub-hosted runners')
    identity = json.loads((ROOT / 'third-party/distribution-build-inputs.json').read_text('utf-8'))
    cargo = Path(os.environ.get('CARGO_HOME') or Path.home() / '.cargo').resolve()
    rustup = Path(os.environ.get('RUSTUP_HOME') or Path.home() / '.rustup').resolve()
    # Hosted images can route these homes through junctions. Publish the real
    # roots so the existing package guard can verify ordinary files end to end.
    os.environ.update(CARGO_HOME=str(cargo), RUSTUP_HOME=str(rustup))
    with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8') as output:
        output.write(f'CARGO_HOME={cargo}\nRUSTUP_HOME={rustup}\n')
    with Path(os.environ['GITHUB_PATH']).open('a', encoding='utf-8') as output:
        output.write(str(cargo / 'bin') + '\n')
    nsis = Path(os.environ['LOCALAPPDATA']) / 'tauri/NSIS'
    with tempfile.TemporaryDirectory(prefix='cerulean-reviewed-tools-', dir=os.environ['RUNNER_TEMP']) as directory:
        temporary = Path(directory)
        if not matches(cargo, identity['files']['cargo-home']):
            installer = download('rustup-init.exe', temporary)
            subprocess.run([str(installer), '-y', '--no-modify-path', '--default-toolchain', 'none'],
                           check=True, timeout=180)
        if not matches(cargo, identity['files']['cargo-home']):
            raise RuntimeError('Installed Rust proxies differ from reviewed inputs')
        for item in identity['files']['cargo-home']:
            path = cargo / item['path']
            if path.is_symlink() or getattr(path.lstat(), 'st_file_attributes', 0) & 0x400:
                data = path.read_bytes()
                if len(data) != item['bytes'] or hashlib.sha256(data).hexdigest() != item['sha256']:
                    raise RuntimeError('Linked proxy content differs from reviewed input')
                replacement = path.with_name(path.name + '.cerulean-ordinary')
                with replacement.open('xb') as output:
                    output.write(data)
                # Replace this known proxy link itself, preserving its exact
                # reviewed bytes and leaving the link target untouched.
                os.replace(replacement, path)
        if not nsis.exists():
            archive = download('nsis.zip', temporary)
            plugin = download('nsis_tauri_utils.dll', temporary)
            with zipfile.ZipFile(archive) as bundle:
                if sum(item.file_size for item in bundle.infolist()) > 64 * 1024 * 1024:
                    raise RuntimeError('NSIS archive exceeds the reviewed size bound')
                for item in bundle.infolist():
                    relative = PurePosixPath(item.filename)
                    if (relative.is_absolute() or '..' in relative.parts or '\\' in item.filename
                            or ':' in item.filename or relative.parts[0] != 'nsis-3.11'
                            or stat.S_IFMT(item.external_attr >> 16) == stat.S_IFLNK):
                        raise RuntimeError('Unexpected NSIS archive member')
                    destination = nsis.joinpath(*relative.parts[1:]).resolve()
                    if not destination.is_relative_to(nsis.resolve()):
                        raise RuntimeError('NSIS member escapes the tool directory')
                    if item.is_dir():
                        destination.mkdir(parents=True, exist_ok=True)
                    else:
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        with destination.open('xb') as output:
                            output.write(bundle.read(item))
            target = nsis / 'Plugins/x86-unicode/additional/nsis_tauri_utils.dll'
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as output:
                output.write(plugin.read_bytes())
        if not matches(nsis, identity['files']['tauri-nsis']):
            raise RuntimeError('NSIS cache differs from reviewed inputs')
    print(json.dumps({'passed': True, 'rustProxyFiles': 3, 'nsisFiles': 13,
                      'reviewedIdentityChanged': False}))


if __name__ == '__main__':
    provision()
