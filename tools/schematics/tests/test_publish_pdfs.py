"""Exercise release publication with real disposable Git histories and mocked GitHub writes."""
import hashlib
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("publish_pdfs", Path(__file__).resolve().parents[1] / "publish_pdfs.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


def git(repo, *args):
    result = subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)
    return result.stdout.decode().strip()


class PublisherTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="schematic-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.remote = self.root / "remote.git"
        self.repo = self.root / "checkout"
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        git(self.root, "init", "--bare", "--initial-branch=main", str(self.remote))
        git(self.root, "clone", str(self.remote), str(self.repo))
        git(self.repo, "config", "user.name", "Test author")
        git(self.repo, "config", "user.email", "test@example.com")
        self.write("mainboard/mainboard.kicad_sch", b"source mainboard\n")
        self.write("som/som.kicad_sch", b"source som\n")
        self.write("README.md", b"documentation\n")
        for name in publisher.PDF_NAMES:
            (self.artifacts / name).write_bytes(b"%PDF-1.5\n" + name.encode() + b"\n%%EOF\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-m", "Initial sources")
        git(self.repo, "push", "origin", "main")
        self.source = git(self.repo, "rev-parse", "HEAD")
        self.github_calls = []
        self.notes = []
        self.existing_release = False
        self.lookup_error = None
        self.fail_action = None
        self.during_lookup = None
        original_run = publisher.run

        def fake_github(repo, *command, check=True):
            if command[0] != "gh":
                return original_run(repo, *command, check=check)
            self.github_calls.append(command)
            if command[1:3] == ("release", "view"):
                if self.during_lookup:
                    self.during_lookup()
                if self.lookup_error:
                    return subprocess.CompletedProcess(command, 1, "", self.lookup_error)
                if self.existing_release:
                    return subprocess.CompletedProcess(command, 0, "", "")
                return subprocess.CompletedProcess(command, 1, "", "release not found\n")
            if command[2] == self.fail_action:
                raise subprocess.CalledProcessError(1, command, stderr="GitHub publication failed")
            if "--notes-file" in command:
                self.notes.append(Path(command[command.index("--notes-file") + 1]).read_text())
            return subprocess.CompletedProcess(command, 0, "", "")

        patch = mock.patch.object(publisher, "run", side_effect=fake_github)
        patch.start()
        self.addCleanup(patch.stop)

    def write(self, name, data):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def tip(self):
        return git(self.remote, "rev-parse", "refs/heads/main")

    def publish(self):
        return publisher.publish(self.source, self.artifacts, "test/project", repo=self.repo)

    def advance(self, name="README.md", text=b"new documentation\n"):
        self.write(name, text)
        git(self.repo, "add", "--", name)
        git(self.repo, "commit", "-m", "Advance branch")
        git(self.repo, "push", "origin", "HEAD:main")
        return self.tip()

    def mutations(self):
        return [command for command in self.github_calls if command[1] == "release" and command[2] != "view"]

    def test_first_release_has_both_assets_exact_source_and_provenance(self):
        self.write("README.md", b"unrelated staged work\n")
        git(self.repo, "add", "README.md")
        self.write("mainboard/mainboard.kicad_sch", b"unsaved local work\n")
        before = git(self.repo, "status", "--porcelain")
        index = (self.repo / ".git" / "index").read_bytes()
        self.assertEqual(self.publish(), "published")
        self.assertEqual(self.tip(), self.source)
        self.assertEqual(git(self.repo, "status", "--porcelain"), before)
        self.assertEqual((self.repo / ".git" / "index").read_bytes(), index)
        command, = self.mutations()
        self.assertEqual(command[:4], ("gh", "release", "create", "schematic-pdfs"))
        self.assertEqual(command[command.index("--target") + 1], self.source)
        self.assertIn("--prerelease", command)
        self.assertIn("--latest=false", command)
        self.assertIn(self.source, self.notes[0])
        for name in publisher.PDF_NAMES:
            path = self.artifacts / name
            self.assertIn(str(path), command)
            self.assertIn(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + name, self.notes[0])

    def test_existing_or_draft_release_uploads_both_assets_before_publishing(self):
        self.existing_release = True
        self.assertEqual(self.publish(), "published")
        upload, edit = self.mutations()
        self.assertEqual(upload[:4], ("gh", "release", "upload", "schematic-pdfs"))
        self.assertIn("--clobber", upload)
        for name in publisher.PDF_NAMES:
            self.assertIn(str(self.artifacts / name), upload)
        self.assertEqual(edit[:4], ("gh", "release", "edit", "schematic-pdfs"))
        self.assertIn("--latest=false", edit)
        self.assertIn("--draft=false", edit)
        self.assertIn(self.source, self.notes[0])
        self.assertEqual(self.tip(), self.source)

    def test_documentation_advance_allows_publication_without_git_changes(self):
        latest = self.advance()
        self.assertEqual(self.publish(), "published")
        self.assertEqual(self.tip(), latest)

    def test_new_rendering_inputs_skip_publication(self):
        paths = [
            "mainboard/mainboard.kicad_sch", "som/som.kicad_pro",
            "library/new.kicad_sym", "library/new.kicad_wks",
            "mainboard/sym-lib-table", "tools/schematics/new.py",
            ".github/workflows/schematic-pdfs.yml", ".gitattributes",
        ]
        for index, name in enumerate(paths):
            with self.subTest(name=name):
                latest = self.advance(name, f"changed input {index}\n".encode())
                self.assertEqual(self.publish(), "superseded")
                self.assertEqual(self.tip(), latest)
                self.assertEqual(self.github_calls, [])

    def test_same_inputs_on_replaced_branch_history_are_rejected(self):
        replacement = git(self.repo, "commit-tree", self.source + "^{tree}", "-m", "New history")
        git(self.repo, "push", "--force", "origin", replacement + ":refs/heads/main")
        self.assertEqual(self.publish(), "superseded")
        self.assertEqual(self.github_calls, [])

    def test_input_race_during_release_lookup_skips_all_writes(self):
        self.during_lookup = lambda: self.advance("som/som.kicad_sch", b"new schematic\n")
        self.assertEqual(self.publish(), "superseded")
        self.assertEqual(self.mutations(), [])

    def test_api_permission_or_network_errors_do_not_create_a_release(self):
        for error in ("gh: Forbidden (HTTP 403)", "connection reset by peer", "gh: Not Found (HTTP 404)"):
            with self.subTest(error=error):
                self.lookup_error = error
                with self.assertRaisesRegex(RuntimeError, "Could not inspect"):
                    self.publish()
                self.assertEqual(self.mutations(), [])

    def test_upload_failure_does_not_update_notes_or_git(self):
        self.existing_release = True
        self.fail_action = "upload"
        with self.assertRaises(subprocess.CalledProcessError):
            self.publish()
        self.assertEqual([call[2] for call in self.mutations()], ["upload"])
        self.assertEqual(self.notes, [])
        self.assertEqual(self.tip(), self.source)

    def test_create_failure_is_reported(self):
        self.fail_action = "create"
        with self.assertRaises(subprocess.CalledProcessError):
            self.publish()
        self.assertEqual([call[2] for call in self.mutations()], ["create"])

    def test_missing_or_invalid_second_pdf_never_writes_to_github(self):
        second = self.artifacts / publisher.PDF_NAMES[1]
        for data in (None, b"failed exporter output", b"%PDF-1.5\ntruncated"):
            with self.subTest(data=data):
                if second.exists():
                    second.unlink()
                if data is not None:
                    second.write_bytes(data)
                with self.assertRaises(ValueError):
                    self.publish()
                self.assertEqual(self.github_calls, [])

    def test_symlink_pdf_is_rejected(self):
        first = self.artifacts / publisher.PDF_NAMES[0]
        first.rename(first.with_suffix(".other"))
        first.symlink_to(first.with_suffix(".other"))
        with self.assertRaisesRegex(ValueError, "regular PDF"):
            self.publish()
        self.assertEqual(self.github_calls, [])


if __name__ == "__main__":
    unittest.main()
