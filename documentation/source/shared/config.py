from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
extensions = ['myst_parser']
source_suffix = {'.md': 'markdown'}
myst_enable_extensions = ['colon_fence', 'deflist']
myst_heading_anchors = 3
html_theme = 'pydata_sphinx_theme'
html_static_path = [str(ROOT / 'source/shared/assets')]
html_css_files = ['am13e.css']
html_logo = str(ROOT / 'source/shared/assets/beagleboard-logo.svg')
html_title = project
html_short_title = project
html_theme_options = {
    'navbar_start': ['brand-header.html'],
    'navbar_center': [],
    'navbar_end': ['search-button', 'theme-switcher'],
    'navbar_persistent': [],
    'show_nav_level': 2,
    'navigation_depth': 2,
    'show_toc_level': 2,
    'secondary_sidebar_items': ['page-toc'],
    'footer_start': ['copyright'],
    'footer_end': [],
    'use_edit_page_button': False,
}
templates_path = [str(ROOT / 'source/shared/templates')]
html_sidebars = {'**': ['manual-nav.html']}
html_show_sourcelink = False
html_copy_source = False
html_last_updated_fmt = None
html_use_index = False
html_domain_indices = False
exclude_patterns = []
master_doc = 'index'
language = 'en'
pygments_style = 'sphinx'
pygments_dark_style = 'monokai'
nitpicky = True
html_show_copyright = False
copyright = ''
