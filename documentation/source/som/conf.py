from pathlib import Path
project = 'AM13E SoM'
exec(compile((Path(__file__).resolve().parent.parent / 'shared/config.py').read_text(), str(Path(__file__).resolve().parent.parent / 'shared/config.py'), 'exec'))
