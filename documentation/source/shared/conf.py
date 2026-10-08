from pathlib import Path

project = 'QuadESC documentation'
release = ''
version = release
configuration = Path(__file__).resolve().with_name('config.py')
exec(compile(configuration.read_text(), str(configuration), 'exec'))
html_sidebars = {'**': []}
html_theme_options['secondary_sidebar_items'] = []
