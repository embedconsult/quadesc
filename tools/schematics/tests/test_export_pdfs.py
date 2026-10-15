"""Guard the narrow date-only comparison used to avoid empty PDF refreshes."""
import importlib.util
from pathlib import Path
import unittest
from unittest import mock
import subprocess
import tempfile

SPEC = importlib.util.spec_from_file_location('export_pdfs', Path(__file__).resolve().parents[1] / 'export_pdfs.py')
EXPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORT)


class PdfComparisonTests(unittest.TestCase):
    def test_actual_kicad_date_only_change_is_unchanged(self):
        old = b'%PDF-1.5\n/CreationDate (D:2026:10:15:10:21:41)\nschematic data'
        new = old.replace(b'10:21:41', b'11:22:42')
        self.assertTrue(EXPORT.same_pdf_content(old, new))

    def test_standard_pdf_date_only_change_is_unchanged(self):
        old = b'%PDF-1.5\n/CreationDate (D:20261015102141)\nschematic data'
        new = old.replace(b'20261015102141', b'20261016112242')
        self.assertTrue(EXPORT.same_pdf_content(old, new))

    def test_schematic_change_with_same_date_must_update(self):
        old = b'%PDF-1.5\n/CreationDate (D:2026:10:15:10:21:41)\nold schematic'
        self.assertFalse(EXPORT.same_pdf_content(old, old.replace(b'old schematic', b'new schematic')))

    def test_other_metadata_change_must_update(self):
        old = b'/CreationDate (D:2026:10:15:10:21:41)\n/Title (old)'
        self.assertFalse(EXPORT.same_pdf_content(old, old.replace(b'(old)', b'(new)')))

    def test_unrecognized_date_is_not_ignored(self):
        self.assertFalse(EXPORT.same_pdf_content(b'/CreationDate (old)', b'/CreationDate (new)'))

    def test_duplicate_date_fields_are_not_ignored(self):
        old = b'/CreationDate (D:20261015102141) /CreationDate (D:20261015102141)'
        self.assertFalse(EXPORT.same_pdf_content(old, old.replace(b'20261015102141', b'20261016112242')))


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
