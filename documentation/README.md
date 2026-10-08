# Documentation source and publishing

Edit the manuals in `source/som/` and `source/quadesc/`. The site landing page is `source/shared/index.md`; theme templates, diagrams and styles are in `source/shared/`. Electrical tables and the thermistor conversion helper are in `reference/`.

The repository stores the editable Markdown, reference tables and diagrams. CI generates the HTML site in `public/`, including a complete printable HTML page for each manual. Generated HTML and build caches are ignored by Git.

## Build and preview

Use Python 3.12 or newer. From the repository root:

```sh
python3 -m venv .venv-docs
. .venv-docs/bin/activate
python -m pip install -r tools/documentation/requirements.txt
python tools/documentation/build.py
python -m http.server 8000 --directory public
```

Open <http://localhost:8000/>. Both manuals also have a `print.html` page; use the browser's print command to print or save a PDF. All runtime assets and diagrams are bundled. Internal URLs are relative so the site works beneath a project URL such as `/quadesc/`.

The build runs Sphinx with warnings treated as errors, then validates internal pages, fragments, downloads and CSS assets. Validation also rejects excluded application handoff material, physical pad coordinates in reference columns, remote runtime assets and links outside the published directory. To check an existing site:

```sh
python tools/documentation/validate.py public
```

Only the six named electrical/reference files in `tools/documentation/build.py` are copied into the website. Add a new public download there deliberately, and link to it from the relevant manual.

## GitLab / OpenBeagle

`.gitlab-ci.yml` builds the site for merge requests, the default branch and manually started pipelines. Merge requests provide a downloadable `public/` artifact for review. The `pages` job publishes the same validated artifact only on the default branch.

Enable CI/CD with a runner that can use the configured Python container. Enable GitLab Pages for the project and locate its URL under **Deploy > Pages**. A self-hosted GitLab administrator must have Pages configured for that instance. The workflow uses the classic `pages` job and `public/` artifact convention to support older installations; it does not require the newer `pages.publish` syntax. See [GitLab Pages documentation](https://docs.gitlab.com/user/project/pages/) and the [CI YAML reference](https://docs.gitlab.com/ci/yaml/).

## GitHub Pages

The GitHub repository is [embedconsult/quadesc](https://github.com/embedconsult/quadesc). `.github/workflows/documentation.yml` builds review artifacts for pull requests and publishes the validated site after the documentation change reaches `main`. The GitLab configuration is also available for an OpenBeagle copy.

On `embedconsult/quadesc`, a repository administrator must configure Pages:

1. In **Settings > Pages > Build and deployment**, choose **GitHub Actions** as the source.
2. Allow the `github-pages` environment to deploy from `main`.
3. Merge the documentation change into `main`, or run the workflow manually on `main`.

Pull requests build and upload a `documentation-site` artifact without deploying. Pushes to `main` and manual runs on `main` build, validate and deploy through the `github-pages` environment. Manual runs on other branches produce review artifacts only. No generated HTML needs to be committed to a `gh-pages` branch. See [GitHub Pages publishing configuration](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site) and [custom Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
