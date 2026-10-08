from pathlib import Path
project = 'QuadESC'
exec(compile((Path(__file__).resolve().parent.parent / 'shared/config.py').read_text(), str(Path(__file__).resolve().parent.parent / 'shared/config.py'), 'exec'))
