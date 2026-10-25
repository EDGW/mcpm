#!/usr/bin/env python3
"""Exercise Unix installation transactions against offline release archives in an isolated home."""

import hashlib
import io
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import unittest


REPO = Path(__file__).resolve().parents[2]
# Cargo owns the program inventory; package manifests carry it to the installers.
METADATA = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--manifest-path', str(REPO / 'mcpm/Cargo.toml'),
    '--no-deps', '--format-version', '1', '--locked',
]))
NAMES = sorted(t['name'] for p in METADATA['packages'] if p['name'] == 'mcpm'
               for t in p['targets'] if t['kind'] == ['bin'])


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
relative=${url#https://github.com/EDGW/mcpm/releases/download/}
cp "$MCPM_TEST_RELEASES/$relative" "$destination"
''')
        self.archive('v0.1.0-dev.26w43c')
        self.archive('v0.1.0-dev.26w43d')

    def shim(self, name, content):
        path = self.shims / name
        path.write_text(content)
        path.chmod(0o755)

    def archive(self, version, missing=None, incorrect=None):
        folder = self.releases / version
        folder.mkdir(exist_ok=True)
        archive = folder / f'{self.asset}.tar.gz'
        manifest = '\n'.join(['format=1', f'version={version}', f'asset={self.asset}',
                              *(f'file={name}' for name in NAMES)]) + '\n'
        with tarfile.open(archive, 'w:gz') as packed:
            for name in ['manifest', *NAMES]:
                if name == missing:
                    continue
                filename = name if name == 'manifest' else f'bin/{name}'
                content = manifest if name == 'manifest' else (
                    f'#!/bin/sh\nif [ "$1" = -V ]; then echo "{name} '
                    f'{"broken" if name == incorrect else version[1:]}"; fi\n'
                )
                info = tarfile.TarInfo(filename)
                data = content.encode()
                info.size = len(data)
                info.mode = 0o644 if name == 'manifest' else 0o755
                packed.addfile(info, io.BytesIO(data))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
        return archive

    def run_installer(self, version, success=True):
        result = subprocess.run(['sh', str(REPO / 'scripts/install.sh'), version],
                                env=self.env, capture_output=True, text=True, timeout=30)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def snapshot(self):
        return {p.name: p.read_bytes() for p in (self.install / 'bin').iterdir()}, (self.install / 'manifest').read_bytes()

    def test_install_upgrade_downgrade_repair_and_owned_uninstall(self):
        data = self.home / '.mcpm'
        data.mkdir()
        (data / 'config').write_text('keep me')
        self.run_installer('v0.1.0-dev.26w43c')
        for name in NAMES:
            self.assertEqual((self.links / name).resolve(), (self.install / 'bin' / name).resolve())
        (self.install / 'bin' / NAMES[0]).unlink()
        self.run_installer('v0.1.0-dev.26w43c')
        self.run_installer('v0.1.0-dev.26w43d')
        self.assertIn('version=v0.1.0-dev.26w43d', (self.install / 'manifest').read_text())
        self.run_installer('v0.1.0-dev.26w43c')
        (self.install / 'bin' / 'personal-file').write_text('keep')
        changed_link = self.links / NAMES[0]
        changed_link.unlink()
        changed_link.symlink_to('/some/other/program')
        self.run_installer('--uninstall')
        self.assertTrue(changed_link.is_symlink())
        self.assertEqual((self.install / 'bin' / 'personal-file').read_text(), 'keep')
        self.assertEqual((data / 'config').read_text(), 'keep me')
        self.assertFalse((self.install / 'manifest').exists())
        self.assertFalse((self.links / NAMES[1]).exists())

    def test_rejects_old_versions_before_download_and_understands_semver(self):
        for version in ['v0.1.0-dev.26w43b', 'v0.1.0-dev.26w42z', 'v0.0.9', 'v0.1.0-alpha', 'v0.1.0-dev', 'nonsense']:
            result = self.run_installer(version, success=False)
            self.assertIn('v0.1.0-dev.26w43c', result.stderr)
            self.assertNotIn('Downloading', result.stdout)
        for version in ['v0.1.0', 'v0.1.0-rc.1', 'v0.1.1-dev.1', 'v1.0.0', 'v0.1.0-dev.26w43c+build.1']:
            self.archive(version)
            self.run_installer(version)

    def test_bad_downloads_and_command_conflicts_preserve_previous_install(self):
        self.run_installer('v0.1.0-dev.26w43c')
        original = self.snapshot()
        archive = self.archive('v0.1.0-dev.26w43d')
        archive.write_bytes(b'corrupt')
        self.run_installer('v0.1.0-dev.26w43d', success=False)
        self.assertEqual(self.snapshot(), original)
        for kwargs in [dict(missing=NAMES[0]), dict(incorrect=NAMES[0])]:
            self.archive('v0.1.0-dev.26w43d', **kwargs)
            self.run_installer('v0.1.0-dev.26w43d', success=False)
            self.assertEqual(self.snapshot(), original)
        self.archive('v0.1.0-dev.26w43d')
        link = self.links / NAMES[0]
        link.unlink()
        link.write_text('foreign')
        self.run_installer('v0.1.0-dev.26w43d', success=False)
        self.assertEqual(link.read_text(), 'foreign')
        self.assertEqual(self.snapshot(), original)

    def test_commit_failure_rolls_back_and_concurrent_install_is_refused(self):
        self.run_installer('v0.1.0-dev.26w43c')
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
        self.run_installer('v0.1.0-dev.26w43d', success=False)
        self.assertEqual(self.snapshot(), original)
        self.assertFalse(list(self.install.glob('.stage.*')))
        (self.install / '.install-lock').mkdir()
        result = self.run_installer('v0.1.0-dev.26w43d', success=False)
        self.assertIn('another installer', result.stderr)
        self.assertTrue((self.install / '.install-lock').exists())
        self.assertEqual(self.snapshot(), original)

    def test_unmanaged_installation_is_never_adopted_or_removed(self):
        (self.install / 'bin').mkdir(parents=True)
        foreign = self.install / 'bin/mcpm'
        foreign.write_text('manual install')
        self.run_installer('v0.1.0-dev.26w43c', success=False)
        self.run_installer('--uninstall')
        self.assertEqual(foreign.read_text(), 'manual install')


if __name__ == '__main__':
    unittest.main()
