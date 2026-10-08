#!/usr/bin/env python3
"""Build and validate the documentation site from Markdown source."""
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from bs4 import BeautifulSoup

from validate import validate_site

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'documentation' / 'source'
OUTPUT = ROOT / 'public'
MANUALS = ('som', 'quadesc')
REFERENCE_FILES = (
    'mainboard-analog-routes.csv',
    'mainboard-connector-pads.csv',
    'quadesc-gpio-map.csv',
    'som-connector-pads.csv',
    'som-gpio-map.csv',
    'thermistor-temperature.py',
)


def sphinx(source, destination, cache, builder='html'):
    subprocess.run([
        sys.executable, '-m', 'sphinx', '-W', '--keep-going', '-n', '-E', '-a',
        '-b', builder, '-d', str(cache), str(source), str(destination),
    ], check=True)


def build():
    # Keep caches and intermediate output outside the published tree. A failed
    # build leaves the previous validated site in place.
    scratch = ROOT / '.docs-build'
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='build-', dir=scratch) as folder:
        temporary = Path(folder)
        site = temporary / 'public'
        sphinx(SOURCE / 'shared', site, temporary / 'cache' / 'landing')
        for manual in MANUALS:
            destination = site / manual
            sphinx(SOURCE / manual, destination, temporary / 'cache' / manual / 'html')
            printed = temporary / 'print' / manual
            sphinx(SOURCE / manual, printed, temporary / 'cache' / manual / 'print', 'singlehtml')
            # A sibling print.html preserves ordinary ../som/ and ../reference/
            # links while singlehtml provides links between sections in one file.
            document = BeautifulSoup((printed / 'index.html').read_text(), 'html.parser')
            for tag in document.find_all(href=True):
                tag['href'] = re.sub(r'^#document-[^#]+#(.+)$', r'#\1', tag['href'])
            for tag in document.find_all('script', src=True):
                tag['src'] = tag['src'].replace(
                    '_static/documentation_options.js', '_static/print_documentation_options.js'
                )
            shutil.copy2(
                printed / '_static' / 'documentation_options.js',
                destination / '_static' / 'print_documentation_options.js',
            )
            (destination / 'print.html').write_text(str(document), encoding='utf-8')
            for assets in ('_images', '_downloads'):
                if (printed / assets).exists():
                    shutil.copytree(printed / assets, destination / assets, dirs_exist_ok=True)
        reference = site / 'reference'
        reference.mkdir()
        for name in REFERENCE_FILES:
            source = ROOT / 'documentation' / 'reference' / name
            if source.is_symlink():
                raise ValueError(f'Reference files must be regular files: {source}')
            shutil.copy2(source, reference / name)
        # The theme bundles an unused Jinja source file under _static; it is not
        # a page or runtime resource. Sphinx's build cache is not published either.
        for pattern in ('webpack-macros.html', '.buildinfo', 'objects.inv'):
            for path in site.rglob(pattern):
                path.unlink()
        (site / '.nojekyll').touch()
        errors = validate_site(site)
        if errors:
            raise ValueError('\n'.join(errors))
        if OUTPUT.is_symlink():
            raise ValueError(f'Refusing to replace a symlink: {OUTPUT}')
        if OUTPUT.exists():
            shutil.rmtree(OUTPUT)
        shutil.move(str(site), OUTPUT)
    print(f'Validated site: {OUTPUT}')


if __name__ == '__main__':
    build()
