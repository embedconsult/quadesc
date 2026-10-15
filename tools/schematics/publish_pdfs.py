#!/usr/bin/env python3
"""Publish only generated schematic PDFs without rebasing or touching source files."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile

PDF_NAMES = ("mainboard_schematic.pdf", "som_schematic.pdf")
# Keep these aligned with .github/workflows/schematic-pdfs.yml.
INPUT_PATHS = (
    ":(glob)**/*.kicad_sch",
    ":(glob)**/*.kicad_pro",
    ":(glob)**/*.kicad_sym",
    ":(glob)**/*.kicad_wks",
    ":(glob)**/sym-lib-table",
    "tools/schematics/",
    ".github/workflows/schematic-pdfs.yml",
    ".gitattributes",
)


def git(repo, *args, env=None, data=None, check=True):
    return subprocess.run(
        ["git", *args], cwd=repo, env=env, input=data,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=check,
    )


def publish(source_sha, pdf_dir, remote="origin", branch="main", repo=None):
    """Return published, unchanged, or superseded; fail after three push races."""
    if not re.fullmatch(r"[0-9a-fA-F]{40}", source_sha):
        raise ValueError("--source-sha must be a full Git commit SHA")
    repo = Path(repo or git(None, "rev-parse", "--show-toplevel").stdout.decode().strip())
    git(repo, "cat-file", "-e", source_sha + "^{commit}")
    git(repo, "check-ref-format", "refs/heads/" + branch)
    pdfs = {}
    for name in PDF_NAMES:
        path = Path(pdf_dir) / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Missing regular PDF artifact: {name}")
        data = path.read_bytes()
        if not data.startswith(b"%PDF-") or not data.rstrip().endswith(b"%%EOF"):
            raise ValueError(f"Invalid PDF artifact: {name}")
        pdfs[name] = data

    env = dict(os.environ)
    env.update({
        "GIT_AUTHOR_NAME": "github-actions[bot]",
        "GIT_AUTHOR_EMAIL": "41898282+github-actions[bot]@users.noreply.github.com",
        "GIT_COMMITTER_NAME": "github-actions[bot]",
        "GIT_COMMITTER_EMAIL": "41898282+github-actions[bot]@users.noreply.github.com",
    })
    blobs = {name: git(repo, "hash-object", "-w", "--stdin", data=data).stdout.decode().strip()
             for name, data in pdfs.items()}
    last_failure = ""
    with tempfile.TemporaryDirectory(prefix="schematic-publish-") as temporary:
        env["GIT_INDEX_FILE"] = str(Path(temporary) / "index")
        for attempt in range(3):
            git(repo, "fetch", "--no-tags", "--", remote, "refs/heads/" + branch)
            latest = git(repo, "rev-parse", "FETCH_HEAD").stdout.decode().strip()
            changed = git(repo, "diff", "--quiet", source_sha, latest, "--", *INPUT_PATHS,
                          check=False)
            if changed.returncode == 1:
                print("Rendering inputs changed on the target branch; a newer source run owns publication.")
                return "superseded"
            if changed.returncode:
                raise RuntimeError(changed.stderr.decode().strip())
            git(repo, "read-tree", latest, env=env)
            for name, blob in blobs.items():
                git(repo, "update-index", "--add", "--cacheinfo", "100644", blob, name, env=env)
            tree = git(repo, "write-tree", env=env).stdout.decode().strip()
            if tree == git(repo, "rev-parse", latest + "^{tree}").stdout.decode().strip():
                print("Schematic PDFs are already current.")
                return "unchanged"
            commit = git(repo, "commit-tree", tree, "-p", latest, "-m",
                         "Regenerate schematic PDFs", env=env).stdout.decode().strip()
            pushed = git(repo, "push", "--", remote, commit + ":refs/heads/" + branch,
                         check=False)
            if pushed.returncode == 0:
                print(f"Published regenerated schematic PDFs to {remote}/{branch}.")
                return "published"
            last_failure = pushed.stderr.decode().strip()
    raise RuntimeError("Could not publish schematic PDFs after 3 attempts; rerun the workflow.\n"
                       + last_failure)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--pdf-dir", required=True, type=Path)
    parser.add_argument("--remote", default="origin")
    parser.add_argument("--branch", default="main")
    args = parser.parse_args()
    try:
        publish(args.source_sha, args.pdf_dir, args.remote, args.branch)
    except (ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        if isinstance(error, subprocess.CalledProcessError):
            parser.exit(1, "Git publication failed: " + error.stderr.decode().strip() + "\n")
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
