"""Small manual Windows release with source/Cargo/PE freshness provenance."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import shutil
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
COMMAND = ['cargo', '+1.98.0', 'build', '--release', '--locked',
           '-p', 'martypc_eframe', '--message-format=json']
MAX_BYTES = 128_000_000


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def source(root):
    git = lambda *args: subprocess.check_output(['git', *args], cwd=root).decode('utf8').strip()
    status = git('status', '--porcelain')
    require(not status, 'release source checkout is dirty: '+status)
    files = {}
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=root).decode('utf8')
    for name in filter(None, names.split('\0')):
        path = root/name
        require(path.is_file() and path.resolve().is_relative_to(root), 'release source escapes checkout')
        files[name] = sha(path.read_bytes())
    return dict(head=git('rev-parse', 'HEAD'), tree=git('rev-parse', 'HEAD^{tree}'), files=files)


def pe64(raw):
    require(64 <= len(raw) <= MAX_BYTES and raw[:2] == b'MZ', 'release product is not bounded DOS/PE')
    at = struct.unpack_from('<I', raw, 0x3c)[0]
    require(at+26 <= len(raw) and raw[at:at+4] == b'PE\0\0' and
            struct.unpack_from('<H', raw, at+4)[0] == 0x8664 and
            struct.unpack_from('<H', raw, at+24)[0] == 0x20b, 'release product is not x64 PE32+')


def verify(receipt, raw, expected_source):
    require(receipt['schema'] == 'martypc-windows-release/1' and receipt['status'] == 'PASS'
            and receipt['exit_code'] == 0 and receipt['command'] == COMMAND, 'release command/status differs')
    require('release: 1.98.0\n' in receipt['rustc'] and
            'host: x86_64-pc-windows-msvc\n' in receipt['rustc'], 'release Rust toolchain differs')
    require(receipt['source_before'] == receipt['source_after'] == expected_source and expected_source['files'],
            'release source changed during build')
    require(receipt['before'] == dict(primary_absent=True, deps_absent=True), 'release old products not removed')
    artifact = receipt['artifact']
    profile = artifact['profile']
    root = PureWindowsPath(receipt['source_root'])
    primary = root/'target/release/martypc.exe'
    require(root.is_absolute() and PureWindowsPath(receipt['primary_path']) == primary and
            PureWindowsPath(artifact['target']['src_path']) == root/'crates/bin/martypc_eframe/src/main.rs',
            'release artifact paths differ')
    require(artifact['reason'] == 'compiler-artifact' and artifact['target']['name'] == 'martypc'
            and artifact['target']['kind'] == ['bin'] and artifact['fresh'] is False
            and PureWindowsPath(artifact['executable']) == primary, 'release Cargo artifact identity/freshness differs')
    require(profile['opt_level'] == '3' and profile['debug_assertions'] is False
            and type(profile['debuginfo']) is int and profile['debuginfo'] == 0
            and profile['test'] is False, 'release Cargo profile differs')
    paths = [PureWindowsPath(item['path']) for item in receipt['products']]
    require(len(paths) >= 2 and paths[0] == primary and len(set(paths)) == len(paths)
            and all(path.parent == root/'target/release/deps' and path.name.startswith('martypc')
                    and path.suffix == '.exe' for path in paths[1:]), 'release product pair differs')
    for item in receipt['products']:
        require(item['mtime_ns'] > receipt['started_ns'] and item['bytes'] == len(raw)
                and item['sha256'] == sha(raw), 'release product freshness/hash differs')
    require(receipt['product_sha256'] == sha(raw), 'release payload hash differs')
    pe64(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'nt', 'Windows release requires native Windows')
    out = args.out.resolve()
    require(out.is_relative_to(ROOT/'.ci-release'), 'release output escapes ignored CI workspace')
    require(not out.exists(), 'release output already exists')
    require(not any(value for key, value in os.environ.items() if
            key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET') or
            key.startswith('CARGO_PROFILE_')), 'release environment overrides the pinned profile')
    before_source = source(ROOT)
    rustc = subprocess.check_output(['rustc', '+1.98.0', '-vV'], cwd=ROOT).decode().replace('\r\n', '\n')
    require('release: 1.98.0\n' in rustc and 'host: x86_64-pc-windows-msvc\n' in rustc,
            'release Rust toolchain differs')
    target = ROOT/'target/release'
    primary, deps = target/'martypc.exe', target/'deps'
    old = [primary, *deps.glob('martypc*.exe')]
    for path in old:
        require(path.resolve().is_relative_to(target), 'release cleanup escapes owned target')
        path.unlink(missing_ok=True)
    require(not primary.exists() and not list(deps.glob('martypc*.exe')), 'release old products remain')
    work = ROOT/'.ci-release'
    work.mkdir(exist_ok=True)
    started_ns = time.time_ns()
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'target'))
    with (work/'cargo.jsonl').open('xb') as stdout:
        code = subprocess.call(COMMAND, cwd=ROOT, env=env, stdout=stdout)
    require(code == 0, 'release Cargo build failed; inspect job log and cargo.jsonl')
    rows = [json.loads(line) for line in (work/'cargo.jsonl').read_text(encoding='utf8').splitlines()]
    matches = [row for row in rows if row.get('reason') == 'compiler-artifact'
               and row.get('target', {}).get('name') == 'martypc' and row.get('executable')]
    require(len(matches) == 1, 'release Cargo executable is ambiguous')
    artifact = matches[0]
    require(Path(artifact['target']['src_path']).resolve() == ROOT/'crates/bin/martypc_eframe/src/main.rs',
            'release Cargo source target differs')
    payload = primary.read_bytes()
    candidates = [path for path in deps.glob('martypc*.exe') if sha(path.read_bytes()) == sha(payload)]
    require(candidates, 'release dependency executable is missing or different')
    products = [dict(path=str(path), bytes=path.stat().st_size, mtime_ns=path.stat().st_mtime_ns,
                     sha256=sha(path.read_bytes())) for path in (primary, *sorted(candidates))]
    receipt = dict(schema='martypc-windows-release/1', status='PASS', exit_code=code,
        command=COMMAND, rustc=rustc, source_root=str(ROOT), source_before=before_source, source_after=source(ROOT),
        before=dict(primary_absent=True, deps_absent=True), started_ns=started_ns,
        primary_path=str(primary), products=products, product_sha256=sha(payload), artifact=artifact,
        cargo_log_sha256=sha((work/'cargo.jsonl').read_bytes()))
    verify(receipt, payload, before_source)
    out.mkdir()
    shutil.copyfile(primary, out/'martypc.exe')
    shutil.copyfile(ROOT/'LICENSE', out/'LICENSE')
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf8')
    verify(json.loads((out/'receipt.json').read_bytes()), (out/'martypc.exe').read_bytes(), source(ROOT))
    print('RELEASE_PASS', receipt['product_sha256'], len(payload), flush=True)


if __name__ == '__main__':
    main()
