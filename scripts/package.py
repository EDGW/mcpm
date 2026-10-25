#!/usr/bin/env python3
"""Verify Cargo binary targets and package one complete platform release with SHA-256."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


def package(binary_dir, output_dir, asset, target, tag=None):
    repo = Path(__file__).resolve().parents[1]
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--manifest-path', str(repo / 'mcpm/Cargo.toml'),
        '--no-deps', '--format-version', '1', '--locked',
    ]))
    crate = next(item for item in metadata['packages'] if item['name'] == 'mcpm')
    version = crate['version']
    names = sorted(item['name'] for item in crate['targets'] if item['kind'] == ['bin'])
    windows = 'windows' in target
    filenames = [name + ('.exe' if windows else '') for name in names]
    build_version = None
    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / (asset + ('.zip' if windows else '.tar.gz'))
    with tempfile.TemporaryDirectory(prefix='mcpm-package-') as temporary:
        staging = Path(temporary)
        (staging / 'bin').mkdir()
        for name, filename in zip(names, filenames):
            binary = binary_dir / filename
            actual = subprocess.check_output([str(binary), '-V'], text=True).strip()
            if build_version is None:
                match = re.fullmatch(
                    rf'{re.escape(name)} ({re.escape(version)}(?:\.(?:[0-9a-f]{{6}}(?:\.modified)?|unknown))?)',
                    actual,
                )
                if match is None:
                    raise ValueError(f'{filename}: unexpected version {actual!r}')
                build_version = match[1]
                if tag is not None and tag != f'v{build_version}':
                    raise ValueError(f'release tag {tag!r} does not match binary version v{build_version}')
            if actual != f'{name} {build_version}':
                raise ValueError(f'{filename}: unexpected version {actual!r}')
            subprocess.run([str(binary), '--help'], check=True, stdout=subprocess.DEVNULL)
            if 'linux-musl' in target:
                headers = subprocess.check_output(['readelf', '-l', str(binary)], text=True)
                if 'INTERP' in headers:
                    raise ValueError(f'{filename}: musl executable is not static')
            shutil.copy2(binary, staging / 'bin' / filename)
        # Installers interpret this format through their manifest readers; never source/eval it.
        manifest = ['format=1', f'version=v{build_version}', f'asset={asset}']
        manifest.extend(f'file={filename}' for filename in filenames)
        (staging / 'manifest').write_text('\n'.join(manifest) + '\n', encoding='utf-8')
        files = [staging / 'manifest', *(staging / 'bin' / name for name in filenames)]
        if windows:
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as packed:
                for file in files:
                    packed.write(file, file.relative_to(staging).as_posix())
        else:
            with tarfile.open(archive, 'w:gz') as packed:
                for file in files:
                    packed.add(file, arcname=file.relative_to(staging).as_posix())
    with archive.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(
        f'{digest}  {archive.name}\n', encoding='ascii',
    )
    print(archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', required=True, type=Path)
    parser.add_argument('--output-dir', default=Path('dist'), type=Path)
    parser.add_argument('--asset', required=True)
    parser.add_argument('--target', required=True)
    parser.add_argument('--tag')
    args = parser.parse_args()
    package(args.binary_dir.resolve(), args.output_dir, args.asset, args.target, args.tag)


if __name__ == '__main__':
    main()
