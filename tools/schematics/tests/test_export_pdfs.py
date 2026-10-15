"""Ensure failed exports preserve both previous outputs."""
import importlib.util
from pathlib import Path
import unittest
from unittest import mock
import subprocess
import tempfile

SPEC = importlib.util.spec_from_file_location('export_pdfs', Path(__file__).resolve().parents[1] / 'export_pdfs.py')
EXPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORT)


class ExportFailureTests(unittest.TestCase):
    def test_wrong_kicad_version_leaves_existing_outputs_untouched(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / 'existing'
            output.mkdir()
            previous = output / 'mainboard_schematic.pdf'
            previous.write_bytes(b'existing PDF')
            version = subprocess.CompletedProcess([], 0, stdout='9.0.0\n')
            with mock.patch.object(EXPORT.subprocess, 'run', return_value=version):
                with self.assertRaisesRegex(ValueError, 'Expected KiCad 10.0.6'):
                    EXPORT.export_pdfs(output, local=True)
            self.assertEqual(previous.read_bytes(), b'existing PDF')
            self.assertFalse((output / 'som_schematic.pdf').exists())

    def test_second_export_failure_does_not_replace_either_output(self):
        def cli(command, **kwargs):
            if command[-1] == 'version':
                return subprocess.CompletedProcess(command, 0, stdout='10.0.6\n')
            if command[-1] == 'som/som.kicad_sch':
                raise subprocess.CalledProcessError(1, command)
            Path(command[command.index('-o') + 1]).write_bytes(b'%PDF-1.5\n%%EOF\n')
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / 'existing'
            output.mkdir()
            for _, name in EXPORT.SCHEMATICS:
                (output / name).write_bytes(b'existing ' + name.encode())
            with mock.patch.object(EXPORT.subprocess, 'run', side_effect=cli):
                with self.assertRaises(subprocess.CalledProcessError):
                    EXPORT.export_pdfs(output, local=True)
            for _, name in EXPORT.SCHEMATICS:
                self.assertEqual((output / name).read_bytes(), b'existing ' + name.encode())


if __name__ == '__main__':
    unittest.main()
