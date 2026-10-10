#!/usr/bin/env python3
"""Package an ISO for GitHub's per-asset limit; never modifies the source image."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

CHUNK = 1900 * 1024 * 1024


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def prepare(source, destination, tag, chunk_size=CHUNK):
    if not re.fullmatch(r'v[0-9][0-9A-Za-z._-]*', tag):
        raise ValueError('Expected a v-prefixed release tag')
    images = sorted(source.glob('*.iso'))
    if len(images) != 1:
        raise ValueError('Expected exactly one x86_64 ISO')
    if chunk_size <= 0 or images[0].stat().st_size == 0:
        raise ValueError('ISO and chunk size must be nonempty')
    if destination.exists():
        raise ValueError('Output directory already exists; inspect or remove it first')
    assets = destination / 'assets'
    assets.mkdir(parents=True)
    original = images[0]
    filename = f'sekka-{tag}-x86_64-linux.iso'
    size = original.stat().st_size
    parts = []
    if size <= chunk_size:
        shutil.copyfile(original, assets / filename)
        parts.append(filename)
    else:
        with original.open('rb') as stream:
            index = 0
            while stream.tell() < size:
                name = f'{filename}.part-{index:03d}'
                with (assets / name).open('wb') as part:
                    left = chunk_size
                    while left and (block := stream.read(min(left, 8 * 1024 * 1024))):
                        part.write(block)
                        left -= len(block)
                parts.append(name)
                index += 1
    iso_hash = digest(original)
    (assets / 'SHA256SUMS').write_text(''.join(f'{digest(assets / name)}  {name}\n' for name in parts))
    (assets / 'ISO-SHA256SUMS').write_text(f'{iso_hash}  {filename}\n')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    (assets / 'manifest.json').write_text(json.dumps({
        'tag': tag, 'revision': revision, 'system': 'x86_64-linux',
        'iso': filename, 'bytes': size, 'sha256': iso_hash, 'parts': parts,
    }, indent=2) + '\n')
    shutil.copyfile('flake.lock', assets / 'flake.lock')
    instructions = f'Download `{filename}` and verify it against `ISO-SHA256SUMS`.'
    if len(parts) > 1:
        instructions = f'''The ISO exceeds GitHub's per-file limit. Download **every** `{filename}.part-*` file, `SHA256SUMS`, and `ISO-SHA256SUMS` into one folder. Join the parts before writing a USB drive.

On Linux or macOS:

```sh
shasum -a 256 -c SHA256SUMS
cat {filename}.part-* > {filename}
shasum -a 256 -c ISO-SHA256SUMS
```

On Windows, use PowerShell in that folder:

```powershell
$parts = Get-ChildItem '{filename}.part-*' | Sort-Object Name
$out = [IO.File]::Create((Join-Path $PWD '{filename}'))
try {{
  foreach ($part in $parts) {{
    $inputFile = [IO.File]::OpenRead($part.FullName)
    try {{ $inputFile.CopyTo($out) }} finally {{ $inputFile.Dispose() }}
  }}
}} finally {{ $out.Dispose() }}
Get-FileHash '{filename}' -Algorithm SHA256
```

The resulting SHA-256 must equal `{iso_hash}`.'''
    (destination / 'release-notes.md').write_text(f'''# Sekka {tag}

Experimental preview for x86_64 PCs. Genkan erases an entire selected disk. Read the [limitations](https://github.com/bitemyapp/sekka#why-you-shouldnt-use-sekka-yet) before installing; use a disposable VM or spare computer.

{instructions}

Source revision: `{revision}`. External dependencies: attached `flake.lock`.
The automated component checks and image build passed. This does **not** certify physical hardware compatibility or a complete interactive installation of these exact bytes.
''')


if __name__ == '__main__':
    if len(sys.argv) != 4:
        sys.exit('Usage: prepare-release.py ISO_DIRECTORY OUTPUT_DIRECTORY TAG')
    prepare(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])
