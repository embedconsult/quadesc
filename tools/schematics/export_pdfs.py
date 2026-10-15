#!/usr/bin/env python3
"""Export both complete schematic hierarchies with pinned KiCad 10.0.6."""
import argparse
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
VERSION = '10.0.6'
# Official KiCad image; pin the manifest as well as the human-readable patch tag.
IMAGE = (
    'ghcr.io/kicad/kicad:10.0.6@'
    'sha256:18693567392b80da435f9fa952ce3a3e534c66eb5a6033f5b9c80aa3b19dd3ec'
)
SCHEMATICS = (
    ('mainboard/mainboard.kicad_sch', 'mainboard_schematic.pdf'),
    ('som/som.kicad_sch', 'som_schematic.pdf'),
)


def export_pdfs(output, local=False):
    for source, _ in SCHEMATICS:
        if not (ROOT / source).is_file():
            raise ValueError(f'Missing schematic: {source}')
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='schematic-pdfs-', dir=output.parent) as folder:
        temporary = Path(folder)
        if local:
            command = ['kicad-cli']
            destination = temporary
        else:
            command = [
                'docker', 'run', '--rm', '--user', f'{os.getuid()}:{os.getgid()}',
                '--env', 'HOME=/tmp', '--volume', f'{ROOT}:/workspace:ro',
                '--volume', f'{temporary}:/output:rw', '--workdir', '/workspace',
                '--entrypoint', 'kicad-cli', IMAGE,
            ]
            destination = Path('/output')
        version = subprocess.run(command + ['version'], check=True, capture_output=True, text=True)
        if version.stdout.strip() != VERSION:
            raise ValueError(f'Expected KiCad {VERSION}, found {version.stdout.strip()!r}')
        for source, name in SCHEMATICS:
            subprocess.run(
                command + ['sch', 'export', 'pdf', '-o', str(destination / name), source],
                cwd=ROOT, check=True,
            )
            generated = (temporary / name).read_bytes()
            if not generated.startswith(b'%PDF-'):
                raise ValueError(f'KiCad did not produce a PDF: {name}')
        output.mkdir(parents=True, exist_ok=True)
        for _, name in SCHEMATICS:
            shutil.copyfile(temporary / name, output / name)
            print(f'Exported {output / name}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'build' / 'schematic-pdfs')
    parser.add_argument('--local', action='store_true', help=f'use installed KiCad {VERSION} instead of Docker')
    args = parser.parse_args()
    try:
        export_pdfs(args.output_dir, args.local)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Schematic PDF export failed: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
