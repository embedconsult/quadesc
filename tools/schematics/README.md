# Schematic PDF generation

GitHub Actions exports every hierarchical page for BeagleConnect QuadESC and BeagleConnect Torque when schematic sources, project settings, symbol libraries, drawing sheets, symbol-library tables, export tooling, or this workflow change. Pull requests receive a `schematic-pdfs-<commit>` artifact containing `mainboard_schematic.pdf` and `som_schematic.pdf`. The artifact is available from the **Schematic PDFs** workflow run for seven days.

After a matching change reaches `main`, a separate job refreshes those two tracked PDFs with a bot commit. It compares rendering inputs against the latest `main` before publishing, accepts intervening documentation-only commits, and retries a rejected push up to three times. Results for superseded schematic inputs are skipped. PDF-only bot commits do not trigger another export. The workflow can also be run manually; publication runs only on `main`.

Generation uses the official KiCad 10.0.6 Docker image pinned by its manifest digest. Sources are mounted read-only, and generated files use a separate writable directory. Default KiCad PDF export includes the complete hierarchy, drawing sheets, property popups, and hierarchy links. If only KiCad's creation timestamp differs from a tracked PDF, the existing bytes are retained; other differences cause a refresh.

To reproduce an export with Docker:

```sh
python3 tools/schematics/export_pdfs.py --output-dir build/schematic-pdfs
```

For an installed KiCad 10.0.6, add `--local`. Outputs remain in the requested directory. To refresh tracked PDFs manually after review, copy those two files to the repository root. Run the safeguards with:

```sh
python3 -m unittest discover -s tools/schematics/tests -v
```

Pull-request generation has read-only repository permissions and does not publish changes. Only the publication job on `main` receives `contents: write`. Repository Actions policy must allow that permission, and branch protection must allow the bot commit. If either policy prevents publication, the job fails and retains the generated artifact for manual use; the workflow does not bypass protection or require a personal access token.
