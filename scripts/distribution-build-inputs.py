"""Check reviewed Windows packaging inputs without building or downloading tools.

This pins current local inputs, not the historical build of an upstream DLL.
Run only from distribution-notices.py --check before packaging.
"""
from pathlib import Path, PurePosixPath
import hashlib
import json
import os
import re
import shutil
import subprocess
import threading
import tomllib

MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 128 * 1024 * 1024
PROBE_LIMIT = 8192
PROBE_SECONDS = 10
REQUIRED = {
    'repository': (
        'third-party/distribution-inputs.json', 'third-party/distribution-inputs.zip',
        'rust-toolchain.toml', '.cargo/config.toml',
        'apps/desktop/src-tauri/installer/hooks.nsh',
        'node_modules/@tauri-apps/cli-win32-x64-msvc/cli.win32-x64-msvc.node',
    ),
    'cargo-home': ('bin/rustc.exe', 'bin/cargo.exe', 'bin/rustup.exe'),
    'rust-sysroot': ('bin/rustc.exe', 'share/doc/rust/COPYRIGHT-library.html'),
    'tauri-nsis': (
        'makensis.exe', 'Bin/makensis.exe', 'Bin/zlib1.dll', 'COPYING', 'nsisconf.nsh',
        'Stubs/lzma_solid-x86-unicode', 'Stubs/uninst',
        'Plugins/x86-unicode/System.dll', 'Plugins/x86-unicode/nsDialogs.dll',
        'Plugins/x86-unicode/StartMenu.dll', 'Plugins/x86-unicode/LangDLL.dll',
        'Plugins/x86-unicode/NSISdl.dll',
        'Plugins/x86-unicode/additional/nsis_tauri_utils.dll',
    ),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode('utf-8')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def path_under(base, name):
    relative = PurePosixPath(name)
    require(isinstance(name, str) and name and not relative.is_absolute()
            and not any(p in ('.', '..') for p in relative.parts)
            and '\\' not in name and ':' not in name, 'Unsafe build-input relative path')
    current = base
    for part in ('', *relative.parts):
        if part:
            current = current / part
        info = current.lstat()
        require(not current.is_symlink() and not getattr(info, 'st_file_attributes', 0) & 0x400,
                'Linked build input is not reviewed: ' + name)
    return current


def read_bounded(base, name, limit=MAX_FILE):
    path = path_under(base, name)
    require(path.is_file() and path.stat().st_size <= limit, 'Invalid build-input size: ' + name)
    with path.open('rb') as source:
        data = source.read(limit + 1)
    require(len(data) <= limit, 'Build input grew beyond limit: ' + name)
    return data


def configuration_digest(root):
    config = json.loads(read_bounded(root, 'apps/desktop/src-tauri/tauri.conf.json', 256 * 1024))
    config.pop('version', None)  # Product-only release bumps do not change attribution.
    return sha(canonical(config))


def route_roots(root, identity, env, which=shutil.which):
    # Do not echo environment values; credentials are neither read nor forwarded.
    denied = {'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTC_BOOTSTRAP',
              'RUSTFLAGS', 'CARGO', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET',
              'CARGO_TARGET_DIR', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER',
              'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER', 'NSISDIR', 'NSIS_DIR', 'XDG_CACHE_HOME'}
    for name, value in env.items():
        upper = name.upper()
        if value and (upper in denied or upper.startswith('TAURI_')
                      or (upper.startswith('RUSTUP_') and upper != 'RUSTUP_HOME')
                      or (upper.startswith('CARGO_') and any(t in upper for t in ('RUSTFLAGS', 'RUSTC', 'WRAPPER', 'LINKER')))):
            raise ValueError('Unreviewed compiler or bundler routing environment')
    home = Path.home()
    cargo = Path(env.get('CARGO_HOME') or home / '.cargo')
    rustup = Path(env.get('RUSTUP_HOME') or home / '.rustup')
    local = env.get('LOCALAPPDATA')
    require(bool(local), 'LOCALAPPDATA is required for this reviewed Tauri cache')
    require(cargo.is_absolute() and rustup.is_absolute() and Path(local).is_absolute(),
            'Build-input roots must be absolute')
    config = tomllib.loads(read_bounded(root, 'rust-toolchain.toml', 64 * 1024).decode('utf-8'))
    rust = identity['rust']
    require(config['toolchain']['channel'] == rust['release'], 'Pinned Rust channel changed')
    require(rust['host'] == 'x86_64-pc-windows-msvc'
            and re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', rust['release']) is not None,
            'Unreviewed Rust host or channel')
    sysroot = rustup / 'toolchains' / (rust['release'] + '-' + rust['host'])
    require(sysroot.is_dir(), 'Reviewed Rust toolchain is not installed; no automatic installation')
    for command in ('rustc', 'cargo'):
        located = which(command, path=env.get('PATH'))
        require(bool(located) and Path(located).absolute() == (cargo / 'bin' / (command + '.exe')).absolute(),
                'Actual PATH route is not the reviewed Rust proxy: ' + command)
    # Cargo may load configuration from its home and every ancestor of each cwd.
    for name in ('config', 'config.toml'):
        require(not (cargo / name).exists(), 'External Cargo configuration requires separate review')
    working = (root, root / 'apps/desktop', root / 'apps/desktop/src-tauri')
    allowed = root / '.cargo/config.toml'
    allowed_toolchain = root / 'rust-toolchain.toml'
    for cwd in working:
        for directory in (cwd, *cwd.parents):
            for name in ('config', 'config.toml'):
                candidate = directory / '.cargo' / name
                require(candidate == allowed or not candidate.exists(),
                        'Additional Cargo configuration requires separate review')
            for name in ('rust-toolchain', 'rust-toolchain.toml'):
                candidate = directory / name
                require(candidate == allowed_toolchain or not candidate.exists(),
                        'Additional Rust toolchain selection requires separate review')
    settings = rustup / 'settings.toml'
    if settings.exists():
        values = tomllib.loads(read_bounded(rustup, 'settings.toml', 128 * 1024).decode('utf-8'))
        for folder in values.get('overrides', {}):
            override = Path(folder).absolute()
            require(not any(cwd.absolute() == override or override in cwd.absolute().parents for cwd in working),
                    'Directory Rustup override requires separate review')
    return {'repository': root, 'cargo-home': cargo, 'rust-sysroot': sysroot,
            'tauri-nsis': Path(local) / 'tauri/NSIS'}


def probe_installed_rustc(executable, arguments, cwd):
    # The absolute executable's digest is checked before calling this. Never run
    # a rustup proxy, cargo, or a downloader. Limit each pipe and wall clock.
    safe_env = {k: v for k, v in os.environ.items()
                if not any(t in k.upper() for t in ('KEY', 'TOKEN', 'SECRET', 'CREDENTIAL', 'AUTH', 'PASSWORD', 'SIGN'))}
    flags = subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0
    process = subprocess.Popen([str(executable), *arguments], cwd=cwd, env=safe_env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, creationflags=flags)
    output = [None, None]

    def capture(index, pipe):
        try:
            output[index] = pipe.read(PROBE_LIMIT + 1)
        finally:
            pipe.close()

    readers = [threading.Thread(target=capture, args=(i, pipe), daemon=True)
               for i, pipe in enumerate((process.stdout, process.stderr))]
    for reader in readers:
        reader.start()
    try:
        code = process.wait(timeout=PROBE_SECONDS)
    except subprocess.TimeoutExpired:
        # This process is our read-only identity query, never a build/user task.
        process.kill()
        process.wait(timeout=PROBE_SECONDS)
        raise ValueError('Installed Rust identity query exceeded its deadline') from None
    finally:
        for reader in readers:
            reader.join(timeout=1)
    require(all(part is not None and len(part) <= PROBE_LIMIT for part in output),
            'Installed Rust identity output exceeds bound')
    require(code == 0 and not output[1], 'Installed Rust identity query failed')
    return output[0].decode('utf-8', errors='strict').strip()


def check(root, env=None, which=shutil.which, probe=probe_installed_rustc):
    root = Path(root).absolute()
    raw = read_bounded(root, 'third-party/distribution-build-inputs.json', 256 * 1024)
    identity = json.loads(raw)
    require(type(identity.get('schema')) is int and identity['schema'] == 1
            and identity.get('upstreamPluginHistoryProven') is False,
            'Unsupported or overclaimed distribution identity')
    require(identity.get('configurationSha256') == configuration_digest(root), 'Bundler configuration changed')
    roots = route_roots(root, identity, os.environ if env is None else env, which)
    require(set(identity['files']) == set(REQUIRED), 'Build-input root set changed')
    total = 0
    hashes = {}
    for name, required in REQUIRED.items():
        records = identity['files'][name]
        require(isinstance(records, list) and len(records) == len(required)
                and {item['path'] for item in records} == set(required), 'Build-input file set changed')
        for item in records:
            data = read_bounded(roots[name], item['path'])
            total += len(data)
            require(total <= MAX_TOTAL, 'Build-input aggregate size exceeds limit')
            require(type(item['bytes']) is int and len(data) == item['bytes']
                    and sha(data) == item['sha256'], 'Build input changed: ' + name + '/' + item['path'])
            hashes[name + '/' + item['path']] = sha(data)
    rust = identity['rust']
    installed = roots['rust-sysroot'] / 'bin/rustc.exe'
    details = probe(installed, ['-vV'], root)
    for key, expected in (('release', rust['release']), ('commit-hash', rust['commit']), ('host', rust['host'])):
        found = re.findall(r'^' + re.escape(key) + r': (.+)$', details, re.MULTILINE)
        require(found == [expected], 'Installed Rust identity changed: ' + key)
    observed_root = Path(probe(installed, ['--print', 'sysroot'], root)).absolute()
    require(observed_root == roots['rust-sysroot'].absolute(), 'Installed Rust sysroot changed')
    return {'status': 'reviewed-build-inputs-match', 'schema': 1, 'files': len(hashes),
            'identitySha256': sha(raw), 'hashedBytes': total, 'networkRequests': 0,
            'upstreamPluginHistoryProven': False}
