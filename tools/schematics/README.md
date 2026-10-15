# Schematic PDF generation

GitHub Actions exports every hierarchical page for BeagleConnect QuadESC and BeagleConnect Torque when schematic sources, project settings, symbol libraries, drawing sheets, symbol-library tables, export tooling, or this workflow change. Pull requests receive a `schematic-pdfs-<commit>` artifact containing `mainboard_schematic.pdf` and `som_schematic.pdf`. The artifact is available from the **Schematic PDFs** workflow run for seven days.

After a matching change reaches `main`, a separate job uploads both PDFs to the rolling [`schematic-pdfs` prerelease](https://github.com/embedconsult/quadesc/releases/tag/schematic-pdfs). It is excluded from the latest product release. Documentation uses these stable asset URLs:

- [BeagleConnect QuadESC schematic](https://github.com/embedconsult/quadesc/releases/download/schematic-pdfs/mainboard_schematic.pdf)
- [BeagleConnect Torque schematic](https://github.com/embedconsult/quadesc/releases/download/schematic-pdfs/som_schematic.pdf)

The first release is created after this workflow reaches `main`; those links become available after that successful run. PDFs are generated outputs and must not be committed to Git. Release notes record the exact source commit and each PDF's SHA-256. The rolling tag and its source archives remain at the initial release commit; use the source commit in the release notes for the current PDFs.

Publication compares rendering inputs against the latest `main`, accepts intervening documentation-only commits, and skips results for superseded inputs or commits removed from branch history. It rechecks immediately before release writes. Main-branch runs are serialized by the workflow concurrency group, so a newer run follows any in-flight publication. Upload failures fail the job; rerunning repairs the release with both generated files. Existing release notes are updated only after both uploads succeed. A retry also publishes a draft left behind by an interrupted first release. The workflow can also be run manually; publication runs only on `main`.

Generation uses the official KiCad 10.0.6 Docker image pinned by its manifest digest. Sources are mounted read-only, and generated files use a separate writable directory. Default KiCad PDF export includes the complete hierarchy, drawing sheets, property popups, and hierarchy links.

To reproduce an export with Docker:

```sh
python3 tools/schematics/export_pdfs.py --output-dir build/schematic-pdfs
```

For an installed KiCad 10.0.6, add `--local`. Outputs remain in the requested directory. Run the safeguards with:

```sh
python3 -m unittest discover -s tools/schematics/tests -v
```

Pull-request generation has read-only repository permissions and does not publish changes. Only the publication job on `main` receives `contents: write` and `GH_TOKEN` from the built-in GitHub Actions token. Checkout does not retain credentials, and the workflow never pushes commits or moves existing Git tags. Repository Actions policy must permit release publication; if it prevents publication, the job fails and retains the generated artifact for manual use. No personal access token is required.
