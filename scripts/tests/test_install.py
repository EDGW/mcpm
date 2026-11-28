#!/usr/bin/env python3
"""Exercise Unix installation transactions against offline release archives in an isolated home."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import threading
import unittest
import zipfile


REPO = Path(__file__).resolve().parents[2]
# Cargo owns the program inventory; package manifests carry it to the installers.
METADATA = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--manifest-path', str(REPO / 'crates/mcpm/Cargo.toml'),
    '--no-deps', '--format-version', '1', '--locked',
]))
NAMES = sorted(t['name'] for p in METADATA['packages'] if p['name'] in {'mcpm', 'mcpmd'}
               for t in p['targets'] if t['kind'] == ['bin'])
AGENT = 'mcloader-injector-agent.jar'
VALIDATOR = '''if [ "$*" = 'debug validate --exact' ]; then
 exec python3 - "$0" <<'PYTHON'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1]).resolve().parents[1]
files = json.loads((root / 'manifest.json').read_text())['files']
assert {file['path'] for file in files} == {f'bin/{p.name}' for p in (root / 'bin').iterdir()}
for file in files:
    assert hashlib.sha256((root / file['path']).read_bytes()).hexdigest() == file['sha256']
print('Validated fixture')
PYTHON
fi
if [ "$*" = 'debug hello' ]; then echo 'Welcome fixture'; fi
'''


class InstallerLifecycle(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='mcpm-install-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.home = self.root / 'home with spaces'
        self.home.mkdir()
        self.shims = self.root / 'commands'
        self.shims.mkdir()
        self.releases = self.root / 'releases'
        self.releases.mkdir()
        system = subprocess.check_output(['uname', '-s'], text=True).strip()
        arch = subprocess.check_output(['uname', '-m'], text=True).strip()
        arch = 'arm64' if arch in ('arm64', 'aarch64') else 'x86_64'
        self.asset = f'mcpm-macos-{arch}' if system == 'Darwin' else f'mcpm-linux-{arch}-musl'
        self.install = self.home / '.local/share/mcpm'
        self.links = self.home / '.local/bin'
        self.record = self.home / '.mcpm/mcpmd.path'
        self.env = dict(os.environ, HOME=str(self.home),
                        PATH=f'{self.shims}{os.pathsep}{os.environ["PATH"]}',
                        MCPM_TEST_RELEASES=str(self.releases))
        self.shim('curl', '''#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
 case "$1" in
  https://*) url=$1 ;;
  --output) shift; destination=$1 ;;
 esac
 shift
done
relative=${url#https://github.com/hitszosa/mcpm/releases/download/}
cp "$MCPM_TEST_RELEASES/$relative" "$destination"
''')
        self.archive('v0.1.0-rc1')
        self.archive('v0.1.0-rc2')

    def shim(self, name, content):
        path = self.shims / name
        path.write_text(content)
        path.chmod(0o755)

    def archive(self, version, missing=None, incorrect=None):
        folder = self.releases / version
        folder.mkdir(exist_ok=True)
        archive = folder / f'{self.asset}.zip'
        files = {f'bin/{name}': (
            f'#!/bin/sh\nif [ "$1" = -V ]; then echo "{name} '
            f'{"broken" if name == incorrect else version[1:]}"; exit; fi\n'
            + (VALIDATOR if name == 'mcpm-tools' else '')
        ).encode() for name in NAMES}
        files[f'bin/{AGENT}'] = b'shipped loader agent'
        manifest = {'format': 1, 'version': version, 'asset': self.asset,
                    'files': [{'path': path, 'sha256': hashlib.sha256(data).hexdigest()}
                              for path, data in files.items()]}
        if missing is not None:
            del files[f'bin/{missing}']
        with zipfile.ZipFile(archive, 'w') as packed:
            packed.writestr('manifest.json', json.dumps(manifest))
            for path, data in files.items():
                packed.writestr(path, data)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
        return archive

    def run_installer(self, version, *options, success=True, answer='yes\n'):
        result = subprocess.run(['sh', str(REPO / 'scripts/install.sh'), version, *options],
                                env=self.env, input=answer, capture_output=True, text=True,
                                start_new_session=True, timeout=30)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def snapshot(self):
        return {p.name: p.read_bytes() for p in (self.install / 'bin').iterdir()}, tuple(
            (self.install / name).read_bytes() for name in ('manifest', 'manifest.json')
        )

    def test_install_upgrade_downgrade_repair_and_owned_uninstall(self):
        data = self.home / '.mcpm'
        data.mkdir()
        (data / 'config').write_text('keep me')
        result = self.run_installer('v0.1.0-rc1')
        self.assertLess(result.stdout.index('Validated fixture'), result.stdout.index('Installed '))
        self.assertLess(result.stdout.index('Installed '), result.stdout.index('Welcome fixture'))
        self.assertFalse((self.links / AGENT).exists())
        self.assertEqual(self.record.read_text(), str(self.install / 'bin/mcpmd') + '\n')
        for name in NAMES:
            self.assertEqual((self.links / name).resolve(), (self.install / 'bin' / name).resolve())
        (self.install / 'bin' / NAMES[0]).unlink()
        self.run_installer('v0.1.0-rc1')
        self.run_installer('v0.1.0-rc2')
        self.assertIn('version=v0.1.0-rc2', (self.install / 'manifest').read_text())
        self.run_installer('v0.1.0-rc1')
        (self.install / 'bin' / 'personal-file').write_text('keep')
        changed_link = self.links / NAMES[0]
        changed_link.unlink()
        changed_link.symlink_to('/some/other/program')
        self.run_installer('--uninstall')
        self.assertTrue(changed_link.is_symlink())
        self.assertEqual((self.install / 'bin' / 'personal-file').read_text(), 'keep')
        self.assertEqual((data / 'config').read_text(), 'keep me')
        self.assertFalse((self.install / 'manifest').exists())
        self.assertFalse((self.install / 'manifest.json').exists())
        self.assertFalse((self.install / 'bin' / AGENT).exists())
        self.assertFalse((self.links / NAMES[1]).exists())
        self.assertFalse(self.record.exists())

    def test_rejects_old_versions_before_download_and_understands_semver(self):
        for version in ['v0.1.0-dev.26w45a', 'v0.1.0-rc0', 'v0.1.0-rc.1', 'v0.0.9', 'v0.1.0-alpha', 'v0.1.0-1', 'nonsense']:
            result = self.run_installer(version, success=False)
            self.assertIn('v0.1.0-rc1', result.stderr)
            self.assertNotIn('Downloading', result.stdout)
        for version in ['v0.1.0', 'v0.1.0-rc2', 'v0.1.0-rc1.1', 'v0.1.1-dev.1', 'v1.0.0', 'v0.1.0-rc1+build.1']:
            self.archive(version)
            self.run_installer(version)

    def test_bad_downloads_and_command_conflicts_preserve_previous_install(self):
        self.run_installer('v0.1.0-rc1')
        original = self.snapshot()
        archive = self.archive('v0.1.0-rc2')
        archive.write_bytes(b'corrupt')
        self.run_installer('v0.1.0-rc2', success=False)
        self.assertEqual(self.snapshot(), original)
        for kwargs in [dict(missing=AGENT), dict(incorrect=NAMES[0])]:
            self.archive('v0.1.0-rc2', **kwargs)
            result = self.run_installer('v0.1.0-rc2', success=False)
            self.assertNotIn('Welcome fixture', result.stdout)
            self.assertEqual(self.snapshot(), original)
        self.archive('v0.1.0-rc2')
        link = self.links / NAMES[0]
        link.unlink()
        link.write_text('foreign')
        self.run_installer('v0.1.0-rc2', success=False)
        self.assertEqual(link.read_text(), 'foreign')
        self.assertEqual(self.snapshot(), original)

    def test_commit_failure_rolls_back_and_concurrent_install_is_refused(self):
        self.run_installer('v0.1.0-rc1')
        foreign_record = b'/custom/daemon with spaces\n'
        self.record.write_bytes(foreign_record)
        original = self.snapshot()
        real_mv = shlex.quote(shutil.which('mv'))
        marker = shlex.quote(str(self.root / 'failed-once'))
        self.shim('mv', f'''#!/bin/sh
case "$1" in
 */new-manifest)
  if [ ! -f {marker} ]; then touch {marker}; exit 1; fi ;;
esac
exec {real_mv} "$@"
''')
        self.run_installer('v0.1.0-rc2', success=False)
        self.assertEqual(self.snapshot(), original)
        self.assertEqual(self.record.read_bytes(), foreign_record)
        self.assertFalse(list(self.install.glob('.stage.*')))
        (self.install / '.install-lock').mkdir()
        result = self.run_installer('v0.1.0-rc2', success=False)
        self.assertIn('another installer', result.stderr)
        self.assertTrue((self.install / '.install-lock').exists())
        self.assertEqual(self.snapshot(), original)

    def test_uninstall_preserves_a_record_pointing_at_another_installation(self):
        self.run_installer('v0.1.0-rc1')
        self.record.write_text('/other/mcpmd\n')
        self.run_installer('--uninstall')
        self.assertEqual(self.record.read_text(), '/other/mcpmd\n')

    def test_failed_fresh_install_leaves_no_daemon_registration(self):
        real_mv = shlex.quote(shutil.which('mv'))
        self.shim('mv', f'''#!/bin/sh
case "$1" in */new-manifest) exit 1 ;; esac
exec {real_mv} "$@"
''')
        self.run_installer('v0.1.0-rc1', success=False)
        self.assertFalse(self.record.exists())
        self.assertFalse((self.install / 'bin').exists())

    def test_replacement_requires_consent_and_removes_all_old_contents(self):
        (self.install / 'bin').mkdir(parents=True)
        foreign = self.install / 'bin/mcpm'
        foreign.write_text('manual install')
        hidden = self.install / '.old-settings'
        hidden.write_text('old installation file')
        for answer in ('no\n', ''):
            result = self.run_installer('v0.1.0-rc1', answer=answer)
            self.assertIn('Installation cancelled', result.stdout)
            self.assertNotIn('Downloading', result.stdout)
            self.assertEqual(foreign.read_text(), 'manual install')
            self.assertTrue(hidden.exists())
        self.run_installer('--uninstall')
        self.assertEqual(foreign.read_text(), 'manual install')
        self.run_installer('v0.1.0-rc1')
        self.assertFalse(hidden.exists())
        self.assertEqual((self.install / 'bin' / AGENT).read_bytes(), b'shipped loader agent')

    def test_proxy_install_and_corrupt_proxied_upgrade_preserve_integrity(self):
        releases = self.releases
        route = '/download/https://github.com/hitszosa/mcpm/releases/download/'

        class Proxy(BaseHTTPRequestHandler):
            def do_GET(self):
                if not self.path.startswith(route):
                    self.send_error(404)
                    return
                try:
                    data = (releases / self.path[len(route):]).read_bytes()
                except FileNotFoundError:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *_):
                pass

        server = ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        # Use real curl; block accidental direct network access while allowing the local proxy.
        self.env.update(PATH=os.environ['PATH'], no_proxy='127.0.0.1',
                        http_proxy='http://127.0.0.1:1', https_proxy='http://127.0.0.1:1',
                        all_proxy='http://127.0.0.1:1')
        prefix = f'http://127.0.0.1:{server.server_port}/download'
        try:
            self.run_installer('--proxy', prefix + '/', 'v0.1.0-rc1')
            original = self.snapshot()
            self.assertEqual(
                subprocess.check_output([self.install / 'bin/mcpm', '-V'], text=True).strip(),
                'mcpm 0.1.0-rc1')
            checksum = self.releases / 'v0.1.0-rc2' / f'{self.asset}.zip.sha256'
            checksum.write_text('0' * 64 + '\n')
            self.run_installer('v0.1.0-rc2', '--proxy', prefix, success=False)
            self.assertEqual(self.snapshot(), original)
        finally:
            server.shutdown()
            thread.join()
            server.server_close()


if __name__ == '__main__':
    unittest.main()
