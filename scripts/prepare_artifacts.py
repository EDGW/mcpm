"""Verify and stage the artifacts pinned in artifacts.toml alongside Cargo binaries."""

import argparse
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import re
import shutil
import tempfile
import tomllib
from urllib.parse import urlsplit
from urllib.request import urlopen


LOCK_PATH = Path(__file__).with_name('artifacts.toml')
TARGET_DIR = (
    Path(os.environ['CARGO_TARGET_DIR'])
    if os.environ.get('CARGO_TARGET_DIR')
    else Path(__file__).resolve().parents[1] / 'target'
)
ARTIFACT_CACHE = TARGET_DIR / 'artifacts'


@dataclass(frozen=True)
class Artifact:
    name: str
    url: str
    sha256: str


def env_var_name(artifact_name):
    return (
        'MCPM_ARTIFACT_'
        + re.sub(r'[^A-Za-z0-9]+', '_', artifact_name).upper()
        + '_SHA256'
    )


def load_artifacts():
    with LOCK_PATH.open('rb') as stream:
        lock = tomllib.load(stream)
    entries = lock.get('artifacts')
    if set(lock) != {'artifacts'} or not isinstance(entries, dict) or not entries:
        raise ValueError(f'{LOCK_PATH}: expected a nonempty [artifacts] table')

    artifacts = []
    names = set()
    env_names = set()
    for name, entry in entries.items():
        if (not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', name)
                or name.endswith('.') or name.casefold() in names):
            raise ValueError(f'{LOCK_PATH}: invalid or duplicate artifact filename {name!r}')
        names.add(name.casefold())
        env_key = env_var_name(name)
        if env_key in env_names:
            raise ValueError(f'{LOCK_PATH}: colliding Cargo env name {env_key!r} for {name!r}')
        env_names.add(env_key)
        if not isinstance(entry, dict) or set(entry) != {'url', 'sha256'}:
            raise ValueError(f'{LOCK_PATH}: {name}: expected url and sha256')
        url, digest = entry['url'], entry['sha256']
        if not isinstance(url, str) or any(c.isspace() or ord(c) < 32 for c in url):
            raise ValueError(f'{LOCK_PATH}: {name}: invalid download URL')
        parsed = urlsplit(url)
        if parsed.scheme != 'https' or not parsed.hostname or parsed.username is not None or parsed.fragment:
            raise ValueError(f'{LOCK_PATH}: {name}: expected an HTTPS URL without credentials or fragment')
        if not isinstance(digest, str) or not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError(f'{LOCK_PATH}: {name}: expected a lowercase SHA-256 digest')
        artifacts.append(Artifact(name, url, digest))
    return artifacts


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def matches_pin(path, digest):
    try:
        if path.is_symlink() or not path.is_file():
            return False
        return sha256(path) == digest
    except FileNotFoundError:
        return False


def cached_artifact(artifact):
    directory = ARTIFACT_CACHE / artifact.sha256
    path = directory / artifact.name
    if matches_pin(path, artifact.sha256):
        return path
    directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.download-', dir=directory) as temporary:
        downloaded = Path(temporary) / artifact.name
        with urlopen(artifact.url, timeout=30) as response, downloaded.open('wb') as stream:
            shutil.copyfileobj(response, stream)
        actual = sha256(downloaded)
        if actual != artifact.sha256:
            raise ValueError(f'{artifact.name}: expected SHA-256 {artifact.sha256}, got {actual}')
        downloaded.replace(path)
    return path


def stage_artifact(output_dir, artifact):
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / artifact.name
    if not matches_pin(path, artifact.sha256):
        cached = cached_artifact(artifact)
        with tempfile.TemporaryDirectory(prefix='.artifact-', dir=output_dir) as temporary:
            staged = Path(temporary) / artifact.name
            shutil.copyfile(cached, staged)
            staged.replace(path)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output_dir', type=Path)
    output_dir = parser.parse_args().output_dir
    for artifact in load_artifacts():
        path = stage_artifact(output_dir, artifact)
        print(f'cargo:rerun-if-changed={path}')
        print(f'cargo:rustc-env={env_var_name(artifact.name)}={artifact.sha256}')


if __name__ == '__main__':
    main()
