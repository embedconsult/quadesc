#!/usr/bin/env python3
"""Publish current schematic PDFs to the rolling GitHub prerelease."""
import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile

PDF_NAMES = ("mainboard_schematic.pdf", "som_schematic.pdf")
RELEASE_TAG = "schematic-pdfs"
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


def run(repo, *command, check=True):
    return subprocess.run(command, cwd=repo, check=check, capture_output=True, text=True)


def inputs_current(repo, source_sha, remote, branch):
    run(repo, "git", "fetch", "--no-tags", "--", remote, "refs/heads/" + branch)
    latest = run(repo, "git", "rev-parse", "FETCH_HEAD").stdout.strip()
    ancestor = run(repo, "git", "merge-base", "--is-ancestor", source_sha, latest, check=False)
    if ancestor.returncode == 1:
        return False
    if ancestor.returncode:
        raise RuntimeError(ancestor.stderr.strip())
    changed = run(repo, "git", "diff", "--quiet", source_sha, latest, "--", *INPUT_PATHS,
                  check=False)
    if changed.returncode not in (0, 1):
        raise RuntimeError(changed.stderr.strip())
    return changed.returncode == 0


def release_exists(repo, repository):
    # The CLI also finds drafts left by an interrupted first release.
    result = run(repo, "gh", "release", "view", RELEASE_TAG, "--repo", repository,
                 "--json", "tagName", check=False)
    if result.returncode == 0:
        return True
    if result.stderr.strip() == "release not found":
        return False
    raise RuntimeError("Could not inspect schematic PDF release: " + result.stderr.strip())


def publish(source_sha, pdf_dir, repository, remote="origin", branch="main", repo=None):
    """Return published or superseded, without creating Git commits or changing tags."""
    if not re.fullmatch(r"[0-9a-fA-F]{40}", source_sha):
        raise ValueError("--source-sha must be a full Git commit SHA")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("--repository must be OWNER/REPO")
    repo = Path(repo or run(None, "git", "rev-parse", "--show-toplevel").stdout.strip())
    run(repo, "git", "cat-file", "-e", source_sha + "^{commit}")
    run(repo, "git", "check-ref-format", "refs/heads/" + branch)
    paths = []
    checksums = []
    for name in PDF_NAMES:
        path = Path(pdf_dir) / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Missing regular PDF artifact: {name}")
        data = path.read_bytes()
        if not data.startswith(b"%PDF-") or not data.rstrip().endswith(b"%%EOF"):
            raise ValueError(f"Invalid PDF artifact: {name}")
        paths.append(str(path.resolve()))
        checksums.append(f"{hashlib.sha256(data).hexdigest()}  {name}")

    if not inputs_current(repo, source_sha, remote, branch):
        print("Rendering inputs changed on main; a newer source run owns publication.")
        return "superseded"
    exists = release_exists(repo, repository)
    # Recheck immediately before writing: main can advance during the API lookup.
    if not inputs_current(repo, source_sha, remote, branch):
        print("Rendering inputs changed before release publication; skipping stale PDFs.")
        return "superseded"
    notes = (
        "Automatically generated schematic PDFs for BeagleConnect QuadESC and BeagleConnect Torque.\n\n"
        f"PDF source commit: [{source_sha}](https://github.com/{repository}/commit/{source_sha}).\n"
        "Generated with the pinned official KiCad 10.0.6 image.\n\n"
        "This rolling prerelease replaces its PDF assets after schematic changes reach main. "
        "The tag and source archives retain the initial release commit; "
        "the PDF source commit above identifies the current assets.\n\n"
        "SHA-256:\n```text\n" + "\n".join(checksums) + "\n```\n"
    )
    with tempfile.TemporaryDirectory(prefix="schematic-release-") as temporary:
        notes_file = Path(temporary) / "notes.md"
        notes_file.write_text(notes)
        metadata = ["--repo", repository, "--title", "Schematic PDFs",
                    "--notes-file", str(notes_file), "--prerelease", "--latest=false"]
        if exists:
            run(repo, "gh", "release", "upload", RELEASE_TAG, *paths,
                "--repo", repository, "--clobber")
            # Do not claim a new source until both PDF uploads have succeeded.
            run(repo, "gh", "release", "edit", RELEASE_TAG, "--draft=false", *metadata)
        else:
            run(repo, "gh", "release", "create", RELEASE_TAG, *paths,
                "--target", source_sha, *metadata)
    print(f"Published schematic PDFs to https://github.com/{repository}/releases/tag/{RELEASE_TAG}.")
    return "published"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--pdf-dir", required=True, type=Path)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--remote", default="origin")
    parser.add_argument("--branch", default="main")
    args = parser.parse_args()
    try:
        publish(args.source_sha, args.pdf_dir, args.repository, args.remote, args.branch)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        if isinstance(error, subprocess.CalledProcessError):
            parser.exit(1, "Schematic PDF publication failed: " + error.stderr.strip() + "\n")
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    main()
