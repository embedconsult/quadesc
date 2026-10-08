#!/usr/bin/env python3
"""Check published links, local assets, reference tables and content exclusions."""
import argparse
import csv
from html.parser import HTMLParser
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET

EXCLUDED_TEXT = re.compile(
    r'\bschema[\s_-]*7\b|\bbench[\s_-]*(?:firmware|demo|application|profile)\b'
    r'|\bunloaded[\s_-]+bench\b|\bABI1\b|\bflight[\s_-]*recorder\b'
    r'|\b(?:footprint[ _-]*origin|terminal[ _-]*anchor|pad[ _-]*cent(?:er|re))\b'
    r'|\b(?:[xy][ /,_-]*[xy]|[xy])[\s_-]+coordinates?\b'
    r'|\+x\s+right\s*,\s*\+y\s+down',
    re.IGNORECASE,
)
EXCLUDED_PATH = re.compile(
    r'(?:^|/)(?:engineering|firmware|bench|provenance|hardware)(?:/|$)'
    r'|schema[ _-]*7|\.(?:zip|tar|tgz|gz|bin|elf|hex|kicad_pcb|kicad_sch|md)$'
    r'|hardware.*\.json$', re.IGNORECASE,
)
COORDINATE_COLUMN = re.compile(
    r'origin|anchor|coordinate|rotation|(?:^|[_ /-])[xy](?:$|[_ /-])', re.IGNORECASE,
)
URL_IN_CSS = re.compile(r'url\(\s*[\'"]?([^\'"\)]+)[\'"]?\s*\)')


class Page(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.links = []
        self.assets = []
        self.text = []
        self.skip = 0

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if attrs.get('id'):
            self.ids.add(attrs['id'])
        if tag == 'a' and attrs.get('name'):
            self.ids.add(attrs['name'])
        if tag in ('script', 'style'):
            self.skip += 1
        if attrs.get('href'):
            target = self.assets if tag == 'link' else self.links
            target.append(attrs['href'])
        if attrs.get('src'):
            self.assets.append(attrs['src'])
        if attrs.get('action'):
            self.links.append(attrs['action'])
        if attrs.get('srcset'):
            self.assets.extend(part.strip().split()[0] for part in attrs['srcset'].split(','))
        for name in ('alt', 'title', 'aria-label'):
            if attrs.get(name):
                self.text.append(attrs[name])

    def handle_endtag(self, tag):
        if tag in ('script', 'style'):
            self.skip = max(0, self.skip - 1)

    def handle_data(self, data):
        if not self.skip:
            self.text.append(data)


def validate_site(directory):
    root = directory.resolve()
    errors = []
    pages = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            errors.append(f'{path.relative_to(root)}: symlinks are not publishable')
        if not path.is_file():
            continue
        relative = path.relative_to(root)
        if EXCLUDED_PATH.search(relative.as_posix()):
            errors.append(f'{relative}: excluded publication file')
        if path.suffix == '.html':
            page = Page()
            page.feed(path.read_text(encoding='utf-8'))
            pages[path] = page
            match = EXCLUDED_TEXT.search(' '.join(page.text))
            if match:
                errors.append(f'{relative}: excluded content {match.group()!r}')
        elif path.suffix == '.csv':
            with path.open(newline='', encoding='utf-8') as handle:
                rows = list(csv.reader(handle))
            if not rows:
                errors.append(f'{relative}: empty reference table')
                continue
            for header in rows[0]:
                if COORDINATE_COLUMN.search(header):
                    errors.append(f'{relative}: coordinate column {header!r}')
            for index, row in enumerate(rows[1:], start=2):
                if len(row) != len(rows[0]):
                    errors.append(f'{relative}:{index}: inconsistent column count')
            match = EXCLUDED_TEXT.search(path.read_text(encoding='utf-8'))
            if match:
                errors.append(f'{relative}: excluded reference content {match.group()!r}')
        elif path.suffix == '.svg':
            # SVG geometry needs numeric x/y attributes; only reader-visible text
            # and accessible descriptions count as coordinate documentation.
            tree = ET.parse(path)
            visible = ' '.join(
                ''.join(element.itertext()) for element in tree.iter()
                if element.tag.rsplit('}', 1)[-1] in ('text', 'title', 'desc')
            )
            match = EXCLUDED_TEXT.search(visible)
            if match:
                errors.append(f'{relative}: excluded diagram text {match.group()!r}')

    def check(path, value, asset=False):
        parsed = urlsplit(value)
        if parsed.scheme or parsed.netloc:
            if asset and parsed.scheme != 'data':
                errors.append(f'{path.relative_to(root)}: remote asset {value!r}')
            return
        if parsed.path.startswith('/'):
            errors.append(f'{path.relative_to(root)}: URL does not support a Pages subpath: {value!r}')
            return
        target = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
        if not target.is_relative_to(root):
            errors.append(f'{path.relative_to(root)}: link escapes published site: {value!r}')
            return
        if target.is_dir():
            target = target / 'index.html'
        if not target.is_file():
            errors.append(f'{path.relative_to(root)}: missing target: {value!r}')
        elif parsed.fragment and target in pages and unquote(parsed.fragment) not in pages[target].ids:
            errors.append(f'{path.relative_to(root)}: missing fragment: {value!r}')

    for path, page in pages.items():
        for value in page.links:
            check(path, value)
        for value in page.assets:
            check(path, value, asset=True)
    for path in sorted(root.rglob('*.css')):
        for match in URL_IN_CSS.finditer(path.read_text(encoding='utf-8')):
            check(path, match.group(1).strip(), asset=True)
    for expected in ('index.html', 'som/index.html', 'quadesc/index.html', 'som/print.html', 'quadesc/print.html'):
        if not (root / expected).is_file():
            errors.append(f'Missing required page: {expected}')
    return errors


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', nargs='?', type=Path, default=Path('public'))
    arguments = parser.parse_args()
    problems = validate_site(arguments.directory)
    if problems:
        parser.exit(1, '\n'.join(problems) + '\n')
    print(f'Validated site: {arguments.directory.resolve()}')
