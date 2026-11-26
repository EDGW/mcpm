#!/usr/bin/env python3
"""Verify Cargo binary targets and package one complete platform release with SHA-256."""

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import zipfile

from prepare_artifacts import load_artifacts, sha256, stage_artifact


def package(binary_dir, output_dir, asset, target, tag=None):
    repo = Path(__file__).resolve().parents[1]
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--manifest-path', str(repo / 'crates/mcpm/Cargo.toml'),
        '--no-deps', '--format-version', '1', '--locked',
    ]))
    crate = next(item for item in metadata['packages'] if item['name'] == 'mcpm')
    version = crate['version']
    names = sorted(
        item['name']
        for package in metadata['packages']
        if package['name'] in {'mcpm', 'mcpmd'}
        for item in package['targets']
        if item['kind'] == ['bin']
    )
    windows = 'windows' in target
    filenames = [name + ('.exe' if windows else '') for name in names]
    build_version = None
    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / f'{asset}.zip'
    artifacts = load_artifacts()
    binary_names = {name.casefold() for name in filenames}
    if any(artifact.name.casefold() in binary_names for artifact in artifacts):
        raise ValueError('a pinned artifact has the same filename as a Cargo binary')
    with tempfile.TemporaryDirectory(prefix='mcpm-package-') as temporary:
        staging = Path(temporary)
        (staging / 'bin').mkdir()
        files = []
        for artifact in artifacts:
            source = stage_artifact(binary_dir, artifact)
            file = staging / 'bin' / artifact.name
            shutil.copyfile(source, file)
            actual = sha256(file)
            if actual != artifact.sha256:
                raise ValueError(f'{source}: expected SHA-256 {artifact.sha256}, got {actual}')
            files.append((file, actual))
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
            file = staging / 'bin' / filename
            shutil.copy2(binary, file)
            files.append((file, sha256(file)))
        manifest = {
            'format': 1,
            'version': f'v{build_version}',
            'asset': asset,
            'target': target,
            'files': [
                {'path': file.relative_to(staging).as_posix(), 'sha256': digest}
                for file, digest in files
            ],
        }
        manifest_path = staging / 'manifest.json'
        manifest_path.write_bytes((json.dumps(manifest, indent=2) + '\n').encode('utf-8'))
        write_archive(archive, staging, [manifest_path, *(file for file, _ in files)])
        shutil.copyfile(manifest_path, output_dir / manifest_path.name)


def write_archive(archive, staging, files):
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as packed:
        for file in files:
            packed.write(file, file.relative_to(staging).as_posix())
    digest = sha256(archive)
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
