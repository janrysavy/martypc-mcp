"""Portable refusal controls; no Rust build, guest or fixture outside checkout."""
import copy
import os
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import windows_release as release


def fixture():
    raw = bytearray(256)
    raw[:2] = b'MZ'
    struct.pack_into('<I', raw, 0x3c, 0x80)
    raw[0x80:0x84] = b'PE\0\0'
    struct.pack_into('<H', raw, 0x84, 0x8664)
    struct.pack_into('<H', raw, 0x98, 0x20b)
    raw = bytes(raw)
    root = 'D:/ci/martypc'
    primary = root+'/target/release/martypc.exe'
    products = [dict(path=path, bytes=len(raw), sha256=release.sha(raw), mtime_ns=101)
                for path in (primary, root+'/target/release/deps/martypc-1234.exe')]
    source = dict(head='a'*40, tree='b'*40, files={'Cargo.lock': 'c'*64})
    receipt = dict(schema='martypc-windows-release/1', status='PASS', exit_code=0,
        command=release.COMMAND, rustc='release: 1.98.0\nhost: x86_64-pc-windows-msvc\n',
        source_root=root, source_before=copy.deepcopy(source), source_after=copy.deepcopy(source),
        before=dict(primary_absent=True, deps_absent=True), started_ns=100,
        primary_path=primary, products=products, product_sha256=release.sha(raw),
        artifact=dict(reason='compiler-artifact', target=dict(name='martypc', kind=['bin'],
            src_path=root+'/crates/bin/martypc_eframe/src/main.rs'), fresh=False, executable=primary,
            profile=dict(opt_level='3', debug_assertions=False, debuginfo=0, test=False)))
    return receipt, raw, source


class ReleaseWitness(unittest.TestCase):
    def test_checkout_time_configuration_preserves_git_blob_bytes(self):
        base = release.ROOT/'.ci-release'
        base.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=base) as tmp:
            path = Path(tmp)
            env = dict(os.environ, GIT_CONFIG_COUNT='2', GIT_CONFIG_KEY_0='core.autocrlf',
                       GIT_CONFIG_VALUE_0='false', GIT_CONFIG_KEY_1='core.eol', GIT_CONFIG_VALUE_1='lf')
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=path, env=env, stderr=subprocess.DEVNULL)
            git('init', '-q')
            git('config', 'core.autocrlf', 'true')
            (path/'input.txt').write_bytes(b'canonical\n')
            git('add', 'input.txt')
            git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'fixture')
            (path/'input.txt').unlink()
            git('checkout-index', '--all', '--force')
            self.assertEqual((path/'input.txt').read_bytes(), git('show', 'HEAD:input.txt'))
            self.assertEqual(git('status', '--porcelain'), b'')

    def test_positive_json_round_trip(self):
        import json
        receipt, raw, source = fixture()
        release.verify(json.loads(json.dumps(receipt)), raw, source)

    def test_wrong_profile_or_cargo_freshness_refuses(self):
        for field, value in (('fresh', True), ('opt_level', '0'), ('debug_assertions', True),
                             ('debuginfo', 2), ('debuginfo', False), ('test', True)):
            receipt, raw, source = fixture()
            target = receipt['artifact'] if field == 'fresh' else receipt['artifact']['profile']
            target[field] = value
            with self.subTest(field=field, value=value), self.assertRaisesRegex(ValueError, 'freshness|profile'):
                release.verify(receipt, raw, source)

    def test_stale_or_missing_product_refuses(self):
        for change in ('old-primary', 'old-deps', 'mtime', 'hash', 'size', 'missing'):
            receipt, raw, source = fixture()
            if change.startswith('old-'):
                receipt['before']['primary_absent' if change == 'old-primary' else 'deps_absent'] = False
            elif change == 'missing':
                receipt['products'].pop()
            else:
                receipt['products'][1][{'mtime': 'mtime_ns', 'hash': 'sha256', 'size': 'bytes'}[change]] = {
                    'mtime': 100, 'hash': '0'*64, 'size': 0}[change]
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.verify(receipt, raw, source)

    def test_changed_or_coherently_forged_source_refuses(self):
        receipt, raw, source = fixture()
        receipt['source_after']['tree'] = 'd'*40
        with self.assertRaisesRegex(ValueError, 'source changed'):
            release.verify(receipt, raw, source)
        receipt['source_before'] = copy.deepcopy(receipt['source_after'])
        with self.assertRaisesRegex(ValueError, 'source changed'):
            release.verify(receipt, raw, source)

    def test_wrong_command_toolchain_and_target_refuses(self):
        for change in ('command', 'toolchain', 'source-target', 'cargo-target', 'product-path'):
            receipt, raw, source = fixture()
            if change == 'command': receipt['command'] = ['cargo', 'build']
            elif change == 'toolchain': receipt['rustc'] = 'release: 1.98.0\nhost: i686-pc-windows-msvc\n'
            elif change == 'source-target': receipt['artifact']['target']['src_path'] = 'D:/elsewhere/main.rs'
            elif change == 'cargo-target': receipt['artifact']['executable'] = 'D:/elsewhere/martypc.exe'
            else: receipt['products'][1]['path'] = receipt['primary_path']
            with self.subTest(change=change), self.assertRaises(ValueError):
                release.verify(receipt, raw, source)

    def test_corrupt_payload_and_wrong_pe_architecture_refuse(self):
        for change in ('dos', 'pe-offset', 'signature', 'machine', 'optional-header'):
            receipt, raw, source = fixture()
            raw = bytearray(raw)
            at, value = {'dos': (0, 0), 'pe-offset': (0x3c, 255), 'signature': (0x80, 0),
                         'machine': (0x84, 0), 'optional-header': (0x98, 0)}[change]
            raw[at] = value
            # Re-sign metadata so the independent PE guard must find corruption.
            receipt['product_sha256'] = release.sha(raw)
            for item in receipt['products']: item['sha256'] = release.sha(raw)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, 'DOS/PE|PE32'):
                release.verify(receipt, raw, source)

    def test_clean_source_inventory_keeps_null_delimited_last_entry(self):
        base = release.ROOT/'.ci-release'
        base.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=base) as tmp:
            path = Path(tmp)
            (path/'Cargo.lock').write_bytes(b'locked')
            answers = [b'', b'Cargo.lock\0', b'a'*40+b'\n', b'b'*40+b'\n']
            with patch.object(release.subprocess, 'check_output', side_effect=answers):
                source = release.source(path)
            self.assertEqual(source['files'], {'Cargo.lock': release.sha(b'locked')})
            with patch.object(release.subprocess, 'check_output', return_value=b'M Cargo.lock'):
                with self.assertRaisesRegex(ValueError, 'checkout is dirty: M Cargo.lock'):
                    release.source(path)


if __name__ == '__main__':
    unittest.main()
