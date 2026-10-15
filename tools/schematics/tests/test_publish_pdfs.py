"""Exercise publication against disposable real Git repositories, including push races."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("publish_pdfs", Path(__file__).resolve().parents[1] / "publish_pdfs.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


def git(repo, *args):
    result = subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)
    return result.stdout.decode().strip()


def pdf(label):
    """A small valid one-page PDF fixture."""
    parts = [b"%PDF-1.5\n"]
    bodies = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 792 612] /Contents 4 0 R >>",
        b"<< /Length " + str(len(label) + 3).encode() + b" >>\nstream\n% " + label.encode() + b"\nendstream",
    ]
    offsets = [0]
    for number, body in enumerate(bodies, 1):
        offsets.append(sum(map(len, parts)))
        parts.append(str(number).encode() + b" 0 obj\n" + body + b"\nendobj\n")
    xref = sum(map(len, parts))
    parts.append(b"xref\n0 5\n0000000000 65535 f \n")
    parts.extend(f"{offset:010d} 00000 n \n".encode() for offset in offsets[1:])
    parts.append(f"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return b"".join(parts)


class PublisherTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="schematic-publisher-test-")
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
        self.write("mainboard/mainboard.kicad_sch", b"source mainboard\r\n")
        self.write("som/som.kicad_sch", b"source som\r\n")
        self.write(".gitattributes", b"*.kicad_sch text eol=crlf\n")
        self.write("README.md", b"documentation\n")
        for name in publisher.PDF_NAMES:
            self.write(name, pdf("old " + name))
            (self.artifacts / name).write_bytes(pdf("new " + name))
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-m", "Initial sources")
        git(self.repo, "push", "origin", "main")
        self.source = git(self.repo, "rev-parse", "HEAD")

    def write(self, name, data):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def tip(self):
        return git(self.remote, "rev-parse", "refs/heads/main")

    def publish(self):
        return publisher.publish(self.source, self.artifacts, repo=self.repo)

    def advance(self, name="README.md", text=b"new documentation\n"):
        self.write(name, text)
        git(self.repo, "add", "--", name)
        git(self.repo, "commit", "-m", "Advance branch")
        git(self.repo, "push", "origin", "HEAD:main")
        return self.tip()

    def races(self, count):
        """Advance main inside pre-push, after Git negotiates the old tip."""
        author = self.root / "other-author"
        git(self.root, "clone", str(self.remote), str(author))
        git(author, "config", "user.name", "Other author")
        git(author, "config", "user.email", "other@example.com")
        commits = []
        for index in range(count):
            (author / "README.md").write_text(f"concurrent documentation {index}\n")
            git(author, "add", "README.md")
            git(author, "commit", "-m", f"Concurrent docs {index}")
            commits.append(git(author, "rev-parse", "HEAD"))
        git(author, "push", "origin", "HEAD:refs/heads/race-inputs")
        counter = self.root / "push-count"
        hook = self.repo / ".git" / "hooks" / "pre-push"
        hook.write_text(
            f"#!{sys.executable}\n"
            "from pathlib import Path\nimport subprocess\n"
            f"counter = Path({str(counter)!r})\n"
            "index = int(counter.read_text()) if counter.exists() else 0\n"
            f"commits = {commits!r}\n"
            f"remote = {str(self.remote)!r}\n"
            "if index < len(commits):\n"
            "    subprocess.run(['git', '--git-dir', remote, 'update-ref', "
            "'refs/heads/main', commits[index]], check=True)\n"
            "counter.write_text(str(index + 1))\n"
        )
        hook.chmod(0o755)
        return commits, counter

    def test_noop_keeps_remote_tip(self):
        for name in publisher.PDF_NAMES:
            (self.artifacts / name).write_bytes((self.repo / name).read_bytes())
        self.assertEqual(self.publish(), "unchanged")
        self.assertEqual(self.tip(), self.source)

    def test_publishes_only_pdfs_and_preserves_dirty_worktree_and_index(self):
        self.write("README.md", b"unrelated staged work\n")
        git(self.repo, "add", "README.md")
        self.write("mainboard/mainboard.kicad_sch", b"unsaved local change\n")
        index = (self.repo / ".git" / "index").read_bytes()
        source = (self.repo / "mainboard/mainboard.kicad_sch").read_bytes()
        self.assertEqual(self.publish(), "published")
        changed = git(self.remote, "diff", "--name-only", self.source, self.tip()).splitlines()
        self.assertEqual(changed, list(publisher.PDF_NAMES))
        self.assertEqual((self.repo / ".git" / "index").read_bytes(), index)
        self.assertEqual((self.repo / "mainboard/mainboard.kicad_sch").read_bytes(), source)
        for name in publisher.PDF_NAMES:
            content = subprocess.check_output(["git", "show", f"{self.tip()}:{name}"], cwd=self.remote)
            self.assertEqual(content, (self.artifacts / name).read_bytes())

    def test_docs_advance_is_retained_as_parent(self):
        latest = self.advance()
        self.assertEqual(self.publish(), "published")
        self.assertEqual(git(self.remote, "rev-parse", self.tip() + "^"), latest)
        self.assertEqual(git(self.remote, "show", self.tip() + ":README.md"), "new documentation")

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

    def test_docs_race_retries_without_losing_newer_commit(self):
        commits, counter = self.races(1)
        self.assertEqual(self.publish(), "published")
        self.assertEqual(counter.read_text(), "2")
        self.assertEqual(git(self.remote, "rev-parse", self.tip() + "^"), commits[-1])

    def test_repeated_race_fails_after_three_attempts(self):
        commits, counter = self.races(3)
        with self.assertRaisesRegex(RuntimeError, "after 3 attempts"):
            self.publish()
        self.assertEqual(counter.read_text(), "3")
        self.assertEqual(self.tip(), commits[-1])
        for name in publisher.PDF_NAMES:
            self.assertEqual(git(self.remote, "rev-parse", f"{self.source}:{name}"),
                             git(self.remote, "rev-parse", f"{self.tip()}:{name}"))

    def test_missing_or_invalid_second_pdf_never_partially_publishes(self):
        second = self.artifacts / publisher.PDF_NAMES[1]
        for data in (None, b"failed exporter output", b"%PDF-1.5\ntruncated"):
            with self.subTest(data=data):
                if second.exists():
                    second.unlink()
                if data is not None:
                    second.write_bytes(data)
                with self.assertRaises(ValueError):
                    self.publish()
                self.assertEqual(self.tip(), self.source)

    def test_symlink_pdf_is_rejected(self):
        first = self.artifacts / publisher.PDF_NAMES[0]
        first.unlink()
        first.symlink_to(self.repo / publisher.PDF_NAMES[0])
        with self.assertRaisesRegex(ValueError, "regular PDF"):
            self.publish()
        self.assertEqual(self.tip(), self.source)


if __name__ == "__main__":
    unittest.main()
