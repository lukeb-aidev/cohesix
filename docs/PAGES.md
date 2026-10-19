<!-- Author: Lukas Bower -->
<!-- Purpose: Explain the source, automatic publishing and validation of the Cohesix documentation theme. -->
<!-- Copyright 2026 Lukas Bower -->

# Maintaining the documentation site

[The Cohesix website](https://lukeb-aidev.github.io/cohesix/) renders this
repository's Markdown. Edit `README.md` or a document under `docs/` to change
its content. Keep ordinary GitHub Markdown links and fenced code blocks;
individual documents do not need front matter, theme classes or a second copy
of their content.

## Automatic publication

GitHub Pages publishes the root of `main` through its existing automatic
`pages-build-deployment` workflow. A push to `main` rebuilds the site. The
repository-owned `_config.yml`, `_layouts/default.html`, `_data/navigation.yml`
and `assets/` files participate in every build, so documentation updates retain
the same theme. There is no separate generated-site branch or manual HTML edit
to repeat after publication.

The literal paths in `_config.yml` also exclude unused standalone HTML pages
and unpacked historical bundle trees from publication. Repository sources and
release archives remain intact. Preserve externally used verification endpoints,
linked gallery HTML and the shared font/icon dependencies when reviewing unused
pages. A page without a navigation link can still serve one of these purposes.
The shared `404.html` is a required error endpoint, not an unused content page.

The supported Jekyll plugins render Markdown without front matter, use the
README as the homepage and translate local Markdown links to their generated
HTML destinations. The shared layout derives page titles from rendered headings,
including documents with leading copyright comments. Layout asset and navigation
links use Jekyll's `relative_url` filter to preserve the `/cohesix` project path.
Keep these plugins when changing the layout.

Use ordinary Markdown images for page artwork. The shared stylesheet sizes the
homepage logo responsively and preserves its proportions; deeply indented HTML
inside a Markdown-enabled container can become a code example instead of an image.

## Theme and diagrams

The theme uses SwarmUI's final olive, ivory and gold workbench palette and its
Inter and JetBrains Mono fonts. `assets/css/site.css` adapts the palette and
tokens from `apps/swarmui/frontend/styles/workbench.css` and `styles/tokens.css`
to semantic Markdown. It does not import the application's control layout.
When SwarmUI's design tokens change, update the corresponding site tokens and
run the focused site checks. Fonts, their licence notices and the Cohesix mark
remain in the existing SwarmUI asset directory; the layout references them
directly rather than keeping duplicate copies.

Tables use automatic column sizing with word-based wrapping, including inline
code. Short names keep their natural width while longer descriptions share the
remaining space. Wide tables scroll horizontally in a keyboard-focusable region;
without JavaScript, the table itself remains scrollable. Keep these rules in the
shared stylesheet rather than adding widths to individual Markdown documents. The
static checker compares source pipe tables with their rendered counterparts,
including nested published guides. Keep blank lines around tables and use fenced
blocks for namespace trees. Keep each inline code span on one physical line when
it contains angle-bracket placeholders; otherwise Kramdown can interpret a
placeholder as an HTML block and hide later Markdown. Generated table owners
must emit their own blank lines. The shared configuration enables Markdown
inside HTML disclosure blocks and retains the existing heading-anchor format.
Use `<summary markdown="span">Title</summary>` for disclosure titles so Kramdown
keeps the closing tag instead of nesting later sections inside the title.

`assets/js/site.js` adds the page outline, scrollable table regions and Mermaid
rendering. Normal fenced blocks labelled `mermaid` remain the source for both
GitHub and Pages. The renderer loads only on pages containing diagrams. It uses
Mermaid 11.12.0 from jsDelivr with an exact SHA-384 integrity check, strict
security and the site's colours. Font loading precedes diagram measurement.
Existing flowchart labels use a single literal `\n` for line breaks. A small,
tested adapter translates that notation only inside supported flowchart labels
before rendering; the canonical Markdown and displayed source stay unchanged.
Comments, directives, escaped backslashes and other diagram types are preserved.
Each rendered diagram retains its source in an expandable section. A loading
or syntax failure leaves that source visible and does not hide other diagrams.
Large diagrams fit the reading column and offer an expanded view with native
dialog keyboard controls. Without JavaScript, Markdown, links, navigation and
the original diagram source remain usable. Downloading the renderer requires a
network connection.
Printing an expanded diagram temporarily returns its canvas to the document and
restores the expanded view afterwards, so the diagram also appears in print.

To upgrade Mermaid, verify the exact release against the upstream package,
update both the versioned URL and its integrity digest, and exercise rendering
and failure fallbacks. Do not replace the URL with an unpinned latest version.
The diagram source remains documentation, not evidence of a live Cohesix run.

## Preview and validate

Use Ruby 3.3 and Bundler with the supported GitHub Pages dependencies:

The Gemfile selects GitHub Pages 232. Bundler's local lockfile is ignored:
the existing hosted Pages builder uses its own supported dependency set and
does not install a developer machine's transitive dependency lock.

```bash
BUNDLE_PATH="$PWD/out/pages-gems" bundle install
BUNDLE_PATH="$PWD/out/pages-gems" bundle exec jekyll build \
  --destination out/pages-preview/cohesix
python3 scripts/ci/check_pages.py --site out/pages-preview/cohesix
python3 -m unittest discover -s tests -p test_pages.py
node --test tests/pages/*.test.mjs
python3 -m http.server 4173 --bind 127.0.0.1 --directory out/pages-preview
```

The static checks compare published code fences, images, tables and diagram
sources with their Markdown owners and verify the configured publication scope.
They complement the browser checks below.

Open `http://127.0.0.1:4173/cohesix/`. Check the homepage and nested guides,
ordinary code, tables, images, source links and heading anchors. For tables,
check the homepage terminology and document lists, Host API endpoints, Test Plan
commands and the wide audit registers at desktop and phone widths. Short labels
must not split inside words; wide tables must scroll without widening the page.
Inspect the
architecture diagram and a sequence diagram after fonts load. Check a narrow
phone-sized viewport, keyboard access and the mobile documentation disclosure.
Also check a JavaScript-disabled page and a page with the Mermaid request
blocked: the original diagram source must stay readable. These checks validate
documentation presentation; they do not replace runtime or release tests.

Before publishing, run `scripts/check-generated.sh` and `git diff --check` in
addition to the focused site checks. After pushing, verify the completed Pages
deployment and inspect the public homepage, a nested document and a rendered
diagram. A source commit or successful local preview alone does not establish
that the public site was updated.
