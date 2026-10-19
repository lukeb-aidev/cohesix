# Author: Lukas Bower
# Purpose: Test Pages content and exact XML/text sitemap publication coverage.
# Copyright 2026 Lukas Bower

"""Preserve layout, tables and diagram source in generated documentation."""

from __future__ import annotations

import html
import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "check_pages", ROOT / "scripts/ci/check_pages.py"
)
assert SPEC is not None and SPEC.loader is not None
PAGES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PAGES)

DIAGRAM = 'flowchart LR\n  A["<ready> & safe"] --> B\n'
TABLE = "| Name | Meaning |\n| --- | --- |\n| Queen | Authority |\n"
RENDERED_TABLE = (
    "<table><thead><tr><th>Name</th><th>Meaning</th></tr></thead>"
    "<tbody><tr><td>Queen</td><td>Authority</td></tr></tbody></table>"
)
CANONICAL_ROOT = "https://lukeb-aidev.github.io/cohesix"


def sitemap_xml(urls: list[str]) -> str:
    """Create small, independently specified standard sitemap fixtures."""
    return (
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
        + "".join("<url><loc>" + html.escape(url) + "</loc></url>" for url in urls)
        + "</urlset>"
    )


class MarkdownTableTests(unittest.TestCase):
    """Identify source tables even when missing boundaries break rendering."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.path = Path(self.temporary.name) / "guide.md"

    def find(self, text: str) -> list[int]:
        """Return one-based header lines from a small source document."""
        self.path.write_text(text, encoding="utf-8")
        return PAGES.pipe_table_lines(self.path)

    def test_missing_blank_before_a_table_does_not_hide_it(self) -> None:
        self.assertEqual(self.find("## Register\n" + TABLE), [2])

    def test_optional_outer_pipes_alignment_and_header_only_tables(self) -> None:
        self.assertEqual(self.find(
            "| Only column |\n| :--: |\n\n"
            "Left | Right\n:--- | ---:\n\n"
            "  | First | Second\n  | - | -\n"
        ), [1, 4, 7])

    def test_escaped_pipes_and_backslash_parity_preserve_columns(self) -> None:
        self.assertEqual(self.find(
            "| `a\\|b` | Meaning |\n| --- | --- |\n\n"
            "a\\|b\n---\\|---\n\n"
            "Left\\\\| Right\n--- | ---\n"
        ), [1, 7])

    def test_fenced_and_indented_examples_do_not_declare_tables(self) -> None:
        examples = (
            "````markdown\n" + TABLE + "```\n" + TABLE + "````\n"
            "~~~text\n" + TABLE + "~~~\n"
            "    | Name | Meaning |\n    | --- | --- |\n"
            "\t| Name | Meaning |\n\t| --- | --- |\n\n"
        )
        self.assertEqual(self.find(examples), [])
        self.assertEqual(self.find(examples + TABLE), [20])

    def test_malformed_separators_and_setext_headings_are_not_tables(self) -> None:
        for text in (
            "Name\n---\n",
            "| Name | Meaning |\n| --- | words |\n",
            "| Name | Meaning |\n| --- | --- | --- |\n",
            "| Name | Meaning |\n| --- | |\n",
            "| Name | Meaning |\n\n| --- | --- |\n",
        ):
            with self.subTest(text=text):
                self.assertEqual(self.find(text), [])

    def test_table_body_dashes_do_not_create_another_header(self) -> None:
        self.assertEqual(self.find(
            TABLE + "| Unavailable | Unknown |\n| --- | --- |\n"
        ), [1])


class PagesPublicationTests(unittest.TestCase):
    """Reject missing inputs and corrupted generated documentation."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.site = self.root / "site"
        (self.source / "docs").mkdir(parents=True)
        (self.source / "README.md").write_text(
            "# Overview\n\n```mermaid\n" + DIAGRAM + "```\n",
            encoding="utf-8",
        )
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n", encoding="utf-8"
        )
        files = {
            "assets/css/site.css": "body { color: ivory; }",
            "assets/js/site.js": (
                'import { draw } from "./modules/helper.mjs";'
            ),
            "assets/js/modules/helper.mjs": (
                'export { draw } from "../leaf.mjs";'
            ),
            "assets/js/leaf.mjs": 'export const draw = () => {};',
            "apps/swarmui/frontend/styles/fonts.css": (
                '@font-face {src: url("../assets/fonts/'
                'Inter-Variable.woff2");}'
                '@font-face {src: url("../assets/fonts/'
                'JetBrainsMono-Variable.woff2");}'
            ),
            "apps/swarmui/frontend/assets/icons/cohesix-icon.svg": "<svg/>",
        }
        for name in (
            "Inter-Variable.woff2", "JetBrainsMono-Variable.woff2",
            "INTER_LICENSE.txt", "JETBRAINS_MONO_OFL.txt",
        ):
            key = "apps/swarmui/frontend/assets/fonts/" + name
            files[key] = "retained asset"
        for relative, text in files.items():
            target = self.site / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")
        self.home = self.page(
            "README.md",
            '<h1>Overview</h1><pre><code class="language-mermaid">'
            + html.escape(DIAGRAM) + "</code></pre>",
            "Overview",
        )
        self.guide = self.page("docs/GUIDE.md", "<h1>Guide</h1>", "Guide")
        (self.site / "index.html").write_text(self.home, encoding="utf-8")
        (self.site / "docs").mkdir()
        (self.site / "docs/GUIDE.html").write_text(
            self.guide, encoding="utf-8"
        )

    @staticmethod
    def page(source: str, article: str, title: str) -> str:
        """Provide a tiny Jekyll-shaped page with working nested navigation."""
        return (
            '<html><head><title>' + html.escape(title)
            + ' · Cohesix</title><link rel="icon" '
            'href="/cohesix/apps/swarmui/frontend/assets/icons/'
            'cohesix-icon.svg">'
            '<link rel="stylesheet" '
            'href="/cohesix/apps/swarmui/frontend/styles/fonts.css">'
            '<link rel="stylesheet" href="/cohesix/assets/css/site.css">'
            '<script type="module" '
            'src="/cohesix/assets/js/site.js"></script></head><body>'
            '<a href="#main-content">Skip to content</a>'
            '<header class="site-header">Cohesix</header>'
            '<nav id="site-nav"><a href="/cohesix/">Home</a>'
            '<a href="/cohesix/docs/GUIDE.html">Guide</a></nav>'
            '<main id="main-content"><a class="source-link" '
            'href="https://github.com/lukeb-aidev/cohesix/blob/main/'
            + source + '">View source</a><article id="document-content">'
            + article + '</article></main><nav id="page-outline"></nav>'
            '<footer class="site-footer">Cohesix</footer></body></html>'
        )

    def audit(self) -> PAGES.CheckResult:
        """Supply a valid sitemap fixture for the other publication contracts."""
        urls = []
        for generated in sorted(self.site.rglob("*.html")):
            relative = generated.relative_to(self.site).as_posix()
            if relative == "404.html":
                continue
            route = relative.removesuffix("index.html") if (
                generated.name == "index.html"
            ) else relative
            urls.append(CANONICAL_ROOT + "/" + route)
        (self.site / "sitemap.xml").write_text(sitemap_xml(urls), encoding="utf-8")
        (self.site / "sitemap.txt").write_text("\n".join(urls) + "\n", encoding="utf-8")
        return PAGES.check_site(self.site, self.source)

    def test_valid_nested_site_retains_decoded_source(self) -> None:
        result = self.audit()
        self.assertEqual(result.errors, [])
        self.assertEqual((result.pages, result.diagrams), (2, 1))

    def test_full_publication_check_requires_the_sitemap(self) -> None:
        self.assertEqual(self.audit().errors, [])
        (self.site / "sitemap.xml").unlink()
        failures = PAGES.check_site(self.site, self.source).errors
        self.assertTrue(any("cannot read the generated sitemap" in error
                            for error in failures))

    def test_only_explicitly_excluded_root_guides_may_be_unpublished(self) -> None:
        (self.source / "docs/UNUSED.md").write_text(
            "# Unused\n", encoding="utf-8"
        )
        config = self.source / "_config.yml"
        config.write_text(
            "title: Cohesix\nexclude:\n  - docs/UNUSED.md\n", encoding="utf-8"
        )
        self.assertEqual(self.audit().errors, [])
        config.unlink()
        self.assertTrue(any("docs/UNUSED.html: generated page is missing" in error
                            for error in self.audit().errors))

    def test_excluded_markdown_route_cannot_leak_into_the_site(self) -> None:
        (self.source / "docs/UNUSED.md").write_text(
            "# Unused\n", encoding="utf-8"
        )
        (self.source / "_config.yml").write_text(
            "exclude:\n  - docs/UNUSED.md\n", encoding="utf-8"
        )
        (self.site / "docs/UNUSED.html").write_text(
            self.page("docs/UNUSED.md", "<h1>Unused</h1>", "Unused"),
            encoding="utf-8",
        )
        failures = "\n".join(self.audit().errors)
        self.assertIn("docs/UNUSED.html: excluded output is still published", failures)
        self.assertIn("excluded source docs/UNUSED.md is still published", failures)

    def test_directory_exclusion_does_not_match_an_archive_sibling(self) -> None:
        (self.source / "_config.yml").write_text(
            "exclude:\n  - releases/bundle/ # omit the expanded copy\n"
            "plugins:\n  - jekyll-relative-links\n", encoding="utf-8",
        )
        release = self.site / "releases"
        release.mkdir()
        (release / "bundle.tar.gz").write_text("archive", encoding="utf-8")
        self.assertEqual(self.audit().errors, [])
        excluded = release / "bundle"
        excluded.mkdir()
        self.assertTrue(any("releases/bundle: excluded output" in error
                            for error in self.audit().errors))

    def test_exclusion_prefixes_retain_jekyll_trailing_slash_semantics(self) -> None:
        archive = Path("releases/Bundle.tar.gz")
        descendant = Path("releases/Bundle/README.md")
        for literal, excludes_archive in (
            ("releases/Bundle", True), ("releases/Bundle/", False)
        ):
            with self.subTest(literal=literal):
                (self.source / "_config.yml").write_text(
                    "exclude:\n  - " + literal + "\n", encoding="utf-8"
                )
                exclusions = PAGES.publication_exclusions(self.source)
                self.assertEqual(exclusions, (literal,))
                self.assertEqual(PAGES.is_excluded(archive, exclusions),
                                 excludes_archive)
                self.assertTrue(PAGES.is_excluded(descendant, exclusions))

    def test_source_link_detects_an_excluded_readme_index_alias(self) -> None:
        (self.source / "demo").mkdir()
        (self.source / "demo/README.md").write_text(
            "# Demo\n", encoding="utf-8"
        )
        (self.source / "_config.yml").write_text(
            "exclude:\n  - demo/README.md\n", encoding="utf-8"
        )
        (self.site / "demo").mkdir()
        (self.site / "demo/index.html").write_text(
            self.page("demo/README.md", "<h1>Demo</h1>", "Demo"),
            encoding="utf-8",
        )
        self.assertTrue(any("excluded source demo/README.md is still published"
                            in error for error in self.audit().errors))

    def test_unsupported_exclusion_yaml_fails_explicitly(self) -> None:
        for config in (
            "exclude: [docs/UNUSED.md]\n",
            "exclude:\n  - 'docs/UNUSED.md'\n",
            "exclude:\n  - !!str docs/UNUSED.md\n",
            "exclude:\n  - docs/*.md\n",
            "exclude:\n  - ../private\n",
            "exclude:\n- docs/UNUSED.md\n",
            "exclude:\n  - out\nexclude:\n  - target\n",
            "exclude:\n  - out\n    - target\n",
            "exclude:\n",
        ):
            with self.subTest(config=config):
                (self.source / "_config.yml").write_text(config, encoding="utf-8")
                failures = self.audit().errors
                self.assertEqual(len(failures), 1)
                self.assertIn("_config.yml: line", failures[0])
                self.assertIn("exclude", failures[0])

    def test_all_source_linked_markdown_pages_require_the_shared_layout(self) -> None:
        markdown = self.source / "demo/README.md"
        markdown.parent.mkdir()
        markdown.write_text("# Demo\n\n" + TABLE, encoding="utf-8")
        generated = self.site / "demo/index.html"
        generated.parent.mkdir()
        page = self.page(
            "demo/README.md", "<h1>Demo</h1>" + RENDERED_TABLE, "Demo"
        )
        generated.write_text(page, encoding="utf-8")
        result = self.audit()
        self.assertEqual(result.errors, [])
        self.assertEqual(result.pages, 3)
        generated.write_text(
            page.replace('class="site-header"', 'class="unrelated"'),
            encoding="utf-8",
        )
        self.assertTrue(any("demo/index.html: shared layout is missing" in error
                            for error in self.audit().errors))

    def test_source_linked_html_page_is_checked_without_markdown_parsing(self) -> None:
        (self.source / "404.html").write_text(
            "---\nlayout: default\n---\n<h1>Page not found</h1>\n```\n",
            encoding="utf-8",
        )
        page = self.page("404.html", "<h1>Page not found</h1>", "Page not found")
        (self.site / "404.html").write_text(page, encoding="utf-8")
        result = self.audit()
        self.assertEqual(result.errors, [])
        self.assertEqual(result.pages, 3)
        (self.site / "404.html").write_text(
            page.replace('id="main-content"', 'id="missing-main"'),
            encoding="utf-8",
        )
        self.assertTrue(any("404.html: shared layout is missing" in error
                            for error in self.audit().errors))
        (self.site / "404.html").unlink()
        self.assertTrue(any("404.html: generated page is missing" in error
                            for error in self.audit().errors))

    def test_standalone_verification_and_gallery_html_are_exempt(self) -> None:
        for relative in ("google-verification.html", "docs/gallery/index.html"):
            target = self.site / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("<html><body>Standalone</body></html>", encoding="utf-8")
        self.assertEqual(self.audit().errors, [])

    def test_missing_theme_and_native_font_assets_fail(self) -> None:
        (self.site / "assets/js/site.js").unlink()
        fonts = self.site / "apps/swarmui/frontend/assets/fonts"
        (fonts / "Inter-Variable.woff2").unlink()
        failures = "\n".join(self.audit().errors)
        self.assertIn("script asset", failures)
        self.assertIn("missing theme assets", failures)
        self.assertIn("missing local CSS asset", failures)
        self.assertIn("native font asset or licence is missing", failures)

    def test_missing_transitive_module_cannot_pass(self) -> None:
        (self.site / "assets/js/leaf.mjs").unlink()
        failures = "\n".join(self.audit().errors)
        self.assertIn("local JS module '../leaf.mjs'", failures)
        self.assertIn("imported module is missing", failures)

    def test_module_import_cannot_escape_site(self) -> None:
        (self.root / "outside.mjs").write_text("export {};", encoding="utf-8")
        (self.site / "assets/js/site.js").write_text(
            'import "../../../outside.mjs";', encoding="utf-8"
        )
        self.assertTrue(any("import escapes the generated site" in error
                            for error in self.audit().errors))

    def test_module_script_needs_module_loading_mode(self) -> None:
        (self.site / "index.html").write_text(
            self.home.replace('type="module" ', ""), encoding="utf-8"
        )
        self.assertTrue(any("site.js must load with type=module" in error
                            for error in self.audit().errors))

    def test_module_import_needs_a_valid_browser_url(self) -> None:
        (self.site / "assets/js/.leaf.mjs").write_text(
            "export {};", encoding="utf-8"
        )
        for specifier in ("", ".leaf.mjs", "/assets/js/leaf.mjs"):
            with self.subTest(specifier=specifier):
                (self.site / "assets/js/site.js").write_text(
                    f'import "{specifier}";', encoding="utf-8"
                )
                self.assertTrue(any("local JS module" in error
                                    for error in self.audit().errors))

    def test_browser_title_uses_decoded_heading_text(self) -> None:
        guide = self.page(
            "docs/GUIDE.md",
            "<h1>GPU &amp; AI <code>&lt;ready&gt;</code></h1>",
            "GPU & AI <ready>",
        )
        (self.site / "docs/GUIDE.html").write_text(guide, encoding="utf-8")
        self.assertEqual(self.audit().errors, [])

    def test_generic_or_double_escaped_browser_title_fails(self) -> None:
        for guide in (
            self.guide.replace("<title>Guide · Cohesix", "<title>Cohesix"),
            self.page(
                "docs/GUIDE.md", "<h1>GPU &amp; AI</h1>", "GPU &amp; AI"
            ),
        ):
            with self.subTest(page=guide):
                (self.site / "docs/GUIDE.html").write_text(
                    guide, encoding="utf-8"
                )
                self.assertTrue(any("browser title" in error
                                    for error in self.audit().errors))

    def test_missing_layout_or_page_cannot_pass(self) -> None:
        (self.site / "index.html").write_text(
            "<html><body>Unthemed output</body></html>", encoding="utf-8"
        )
        (self.site / "docs/GUIDE.html").unlink()
        failures = "\n".join(self.audit().errors)
        self.assertIn("shared layout is missing", failures)
        self.assertIn("documentation navigation is empty", failures)
        self.assertIn("generated page is missing", failures)

    def test_swallowed_homepage_table_cannot_pass(self) -> None:
        path = self.source / "README.md"
        path.write_text(path.read_text(encoding="utf-8") + TABLE,
                        encoding="utf-8")
        failures = "\n".join(self.audit().errors)
        self.assertIn("index.html: table count differs from README.md", failures)
        self.assertIn("1 pipe tables expected", failures)
        self.assertIn("0 retained with thead under article#document-content",
                      failures)

    def test_headerless_tables_do_not_change_pipe_table_count(self) -> None:
        path = self.source / "README.md"
        path.write_text(path.read_text(encoding="utf-8") + TABLE,
                        encoding="utf-8")
        (self.site / "index.html").write_text(
            self.home.replace(
                "</article>", RENDERED_TABLE
                + "<table><tbody><tr><td>Headerless</td></tr></tbody></table>"
                + "</article>",
            ), encoding="utf-8",
        )
        self.assertEqual(self.audit().errors, [])

    def test_headerless_table_cannot_replace_a_lost_pipe_table(self) -> None:
        path = self.source / "README.md"
        path.write_text(path.read_text(encoding="utf-8") + TABLE,
                        encoding="utf-8")
        (self.site / "index.html").write_text(
            self.home.replace(
                "</article>",
                "<table><tbody><tr><td>Headerless</td></tr></tbody></table>"
                + "</article>",
            ), encoding="utf-8",
        )
        failures = "\n".join(self.audit().errors)
        self.assertIn("1 pipe tables expected", failures)
        self.assertIn("0 retained with thead", failures)

    def test_tables_outside_the_article_cannot_replace_lost_content(self) -> None:
        path = self.source / "README.md"
        path.write_text(path.read_text(encoding="utf-8") + TABLE,
                        encoding="utf-8")
        (self.site / "index.html").write_text(
            self.home.replace("</body>", RENDERED_TABLE + "</body>"),
            encoding="utf-8",
        )
        self.assertTrue(any("table count differs" in error
                            for error in self.audit().errors))

    def test_published_nested_and_contributing_tables_are_checked(self) -> None:
        for name in ("docs/audit/EXCEPTIONS.md", "CONTRIBUTING.md"):
            with self.subTest(document=name):
                markdown = self.source / name
                markdown.parent.mkdir(parents=True, exist_ok=True)
                markdown.write_text("## Register\n" + TABLE, encoding="utf-8")
                generated = self.site / Path(name).with_suffix(".html")
                generated.parent.mkdir(parents=True, exist_ok=True)
                generated.write_text(
                    '<article id="document-content"><p>Swallowed table</p>'
                    '</article>', encoding="utf-8",
                )
                failures = "\n".join(self.audit().errors)
                self.assertIn("table count differs from " + name, failures)
                self.assertIn("source lines [2]", failures)
                generated.write_text(
                    '<article id="document-content">' + RENDERED_TABLE
                    + "</article>", encoding="utf-8",
                )
                self.assertEqual(self.audit().errors, [])

    def test_unpublished_nested_tables_do_not_require_historical_pages(self) -> None:
        markdown = self.source / "docs/archive/OLD.md"
        markdown.parent.mkdir(parents=True)
        markdown.write_text("# Old\n\n" + TABLE, encoding="utf-8")
        self.assertEqual(self.audit().errors, [])

    def test_navigation_requires_base_and_local_destination(self) -> None:
        for url in (
            "/docs/GUIDE.html", "../docs/GUIDE.html",
            "/cohesix//docs/GUIDE.html",
            "/cohesix/%2e%2e/private.html", "/cohesix/docs/ABSENT.html",
            "https://example.invalid/GUIDE.html",
        ):
            with self.subTest(url=url):
                (self.site / "index.html").write_text(
                    self.home.replace("/cohesix/docs/GUIDE.html", url),
                    encoding="utf-8",
                )
                failures = "\n".join(self.audit().errors)
                self.assertIn("navigation", failures)
                self.assertIn(repr(url), failures)

    def test_mermaid_loss_wrong_selector_and_changed_text_fail(self) -> None:
        for replacement in (
            self.home.replace(
                'class="language-mermaid"', 'class="language-text"'
            ),
            self.home.replace("&lt;ready&gt;", "changed"),
            self.home.replace("<pre><code", "<div><code").replace(
                "</code></pre>", "</code></div>"
            ),
        ):
            with self.subTest(page=replacement):
                (self.site / "index.html").write_text(
                    replacement, encoding="utf-8"
                )
                self.assertTrue(any("Mermaid source differs" in error
                                    for error in self.audit().errors))

    def test_source_link_identifies_the_actual_document(self) -> None:
        (self.site / "docs/GUIDE.html").write_text(
            self.guide.replace(
                "blob/main/docs/GUIDE.md", "blob/main/README.md"
            ),
            encoding="utf-8",
        )
        self.assertTrue(any("View source must link" in error
                            for error in self.audit().errors))

    def test_code_example_does_not_create_a_phantom_diagram(self) -> None:
        path = self.source / "example.md"
        path.write_text(
            "````text\n```mermaid\nexample only\n```\n````\n"
            + "```mermaid\n" + DIAGRAM + "```\n",
            encoding="utf-8",
        )
        self.assertEqual(PAGES.mermaid_sources(path), [DIAGRAM])

    def test_unterminated_source_fence_is_a_publication_error(self) -> None:
        (self.source / "README.md").write_text(
            "# Overview\n```mermaid\n" + DIAGRAM, encoding="utf-8"
        )
        self.assertTrue(any("unterminated Markdown code fence" in error
                            for error in self.audit().errors))

    def test_escaped_disclosure_closing_tags_outside_code_fail(self) -> None:
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", "<h1>Guide</h1><details><summary>More"
                      "&lt;/summary&gt;</summary>&lt;/details&gt;</details>", "Guide"),
            encoding="utf-8",
        )
        failures = "\n".join(self.audit().errors)
        self.assertIn("escaped disclosure closing tags outside pre/code", failures)
        self.assertIn("</summary>", failures)
        self.assertIn("</details>", failures)

    def test_real_disclosures_and_closing_tags_in_code_are_allowed(self) -> None:
        code = "</summary>\n</details>\n"
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n```html\n" + code + "```\n", encoding="utf-8"
        )
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", "<h1>Guide</h1><details><summary>More"
                      "</summary><p>Content</p></details>"
                      "<code>&lt;/summary&gt;</code><pre><code>"
                      + html.escape(code) + "</code></pre>", "Guide"),
            encoding="utf-8",
        )
        self.assertEqual(self.audit().errors, [])

    def test_lost_or_changed_ordinary_fenced_code_cannot_pass(self) -> None:
        code = "printf '<input> & value'\n"
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n```bash\n" + code + "```\n", encoding="utf-8"
        )
        for article in (
            "<h1>Guide</h1><p>Swallowed fence</p>",
            "<h1>Guide</h1><pre><code>changed\n</code></pre>",
            "<h1>Guide</h1><code>" + html.escape(code) + "</code>",
        ):
            with self.subTest(article=article):
                (self.site / "docs/GUIDE.html").write_text(
                    self.page("docs/GUIDE.md", article, "Guide"), encoding="utf-8"
                )
                self.assertTrue(any("fenced code at docs/GUIDE.md:3" in error
                                    for error in self.audit().errors))
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", "<h1>Guide</h1><pre><code><span>printf</span>"
                      + html.escape(code[len("printf"):]) + "</code></pre>", "Guide"),
            encoding="utf-8",
        )
        self.assertEqual(self.audit().errors, [])

    def test_repeated_fenced_code_requires_two_retained_blocks(self) -> None:
        code = "same content\n"
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n```text\n" + code + "```\n\n```text\n" + code + "```\n",
            encoding="utf-8",
        )
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", "<h1>Guide</h1><pre><code>"
                      + code + "</code></pre>", "Guide"), encoding="utf-8",
        )
        self.assertTrue(any("fenced code at docs/GUIDE.md:7" in error
                            for error in self.audit().errors))

    def test_raw_html_and_markdown_images_preserve_resolved_sources(self) -> None:
        for relative in ("assets/raw.svg", "docs/images/my logo.svg"):
            target = self.site / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("<svg/>", encoding="utf-8")
        external = "https://example.com/image.svg?x=1&y=2"
        (self.source / "docs/GUIDE.md").write_text(
            '# Guide\n\n<img src="../assets/raw.svg" alt="Raw">\n\n'
            '![Logo](/docs/images/my%20logo.svg)\n\n'
            '![External](' + external + ')\n', encoding="utf-8",
        )
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", '<h1>Guide</h1>'
                      '<img src="/cohesix/assets/raw.svg" alt="Raw">'
                      '<img src="images/my logo.svg" alt="Logo">'
                      '<img src="' + html.escape(external) + '" alt="External">',
                      "Guide"), encoding="utf-8",
        )
        self.assertEqual(self.audit().errors, [])

    def test_escaped_or_replaced_article_image_cannot_pass(self) -> None:
        raw_image = '<img src="image.svg" alt="Logo">'
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n" + raw_image + "\n", encoding="utf-8"
        )
        for relative in ("docs/image.svg", "docs/different.svg"):
            (self.site / relative).write_text("<svg/>", encoding="utf-8")
        for article in (
            '<h1>Guide</h1><pre><code>' + html.escape(raw_image) + '</code></pre>',
            '<h1>Guide</h1><img src="different.svg" alt="Wrong logo">',
        ):
            with self.subTest(article=article):
                (self.site / "docs/GUIDE.html").write_text(
                    self.page("docs/GUIDE.md", article, "Guide"), encoding="utf-8"
                )
                self.assertTrue(any("article images differ from docs/GUIDE.md"
                                    in error for error in self.audit().errors))

    def test_image_examples_inside_fences_and_inline_code_are_ignored(self) -> None:
        example = '<img src="missing.svg">\n![Example](missing.png)\n'
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n```html\n" + example + "```\n\n"
            '`<img src="missing.svg">` and `![Example](missing.png)`\n',
            encoding="utf-8",
        )
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", "<h1>Guide</h1><pre><code>"
                      + html.escape(example) + "</code></pre>"
                      '<code>&lt;img src="missing.svg"&gt;</code>'
                      '<code>![Example](missing.png)</code>', "Guide"),
            encoding="utf-8",
        )
        self.assertEqual(self.audit().errors, [])

    def test_matching_image_urls_still_require_the_local_file(self) -> None:
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n![Logo](missing.svg)\n", encoding="utf-8"
        )
        (self.site / "docs/GUIDE.html").write_text(
            self.page("docs/GUIDE.md", '<h1>Guide</h1>'
                      '<img src="/cohesix/docs/missing.svg" alt="Logo">', "Guide"),
            encoding="utf-8",
        )
        self.assertTrue(any("article image '/cohesix/docs/missing.svg': "
                            "destination is missing" in error
                            for error in self.audit().errors))

    def test_unsupported_reference_images_fail_explicitly(self) -> None:
        (self.source / "docs/GUIDE.md").write_text(
            "# Guide\n\n![Logo][logo]\n\n[logo]: image.svg\n", encoding="utf-8"
        )
        self.assertTrue(any("unsupported image syntax" in error
                            for error in self.audit().errors))


class SitemapTests(unittest.TestCase):
    """Keep exact canonical coverage tied to current output rather than a count."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.site = self.root / "site"
        self.source.mkdir()
        (self.source / "_config.yml").write_text(
            "url: https://lukeb-aidev.github.io\nbaseurl: /cohesix\n",
            encoding="utf-8",
        )
        for relative in (
            "index.html", "docs/GUIDE.html", "examples/index.html",
            "docs/images/walkthrough.html", "404.html", "google1234abcd.html",
        ):
            target = self.site / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            text = (
                "google-site-verification: " + relative
                if relative.startswith("google")
                else "<html><body>Current content</body></html>"
            )
            target.write_text(text, encoding="utf-8")
        self.urls = [
            CANONICAL_ROOT + "/", CANONICAL_ROOT + "/docs/GUIDE.html",
            CANONICAL_ROOT + "/examples/",
            CANONICAL_ROOT + "/docs/images/walkthrough.html",
        ]
        self.write(self.urls)

    def write(self, urls: list[str]) -> None:
        """Write the exact URLs selected by the test rather than deriving them."""
        (self.site / "sitemap.xml").write_text(sitemap_xml(urls), encoding="utf-8")
        (self.site / "sitemap.txt").write_text("\n".join(urls) + "\n", encoding="utf-8")

    def audit(self) -> list[str]:
        """Check XML and URL coverage independently of themed-page fixtures."""
        return PAGES.check_sitemap(self.site, self.source, "/cohesix")

    def test_canonical_content_includes_gallery_and_directory_indexes(self) -> None:
        self.assertEqual(self.audit(), [])
        self.assertEqual(
            PAGES.sitemap_content_urls(
                self.site, "https://lukeb-aidev.github.io", "/cohesix"
            ), set(self.urls),
        )

    def test_missing_text_sitemap_cannot_pass(self) -> None:
        (self.site / "sitemap.txt").unlink()
        self.assertTrue(any("sitemap.txt" in error for error in self.audit()))

    def test_text_sitemap_requires_plain_utf8_url_lines(self) -> None:
        for text in (
            "", "# Sitemap\n" + "\n".join(self.urls),
            "\ufeff" + "\n".join(self.urls),
            "\n".join(self.urls) + "\n\n",
            " " + "\n".join(self.urls),
            "\t" + "\n".join(self.urls),
            "\n".join(self.urls) + " \n",
            "<urlset>" + self.urls[0] + "</urlset>\n",
        ):
            with self.subTest(text=text):
                (self.site / "sitemap.txt").write_text(text, encoding="utf-8")
                self.assertTrue(any(error.startswith("sitemap.txt:")
                                    for error in self.audit()))
        (self.site / "sitemap.txt").write_bytes(b"\xff\xfeinvalid")
        self.assertTrue(any("cannot read the UTF-8 text sitemap" in error
                            for error in self.audit()))

    def test_text_sitemap_duplicates_fail_even_when_xml_matches_its_set(self) -> None:
        (self.site / "sitemap.txt").write_text(
            "\n".join(self.urls + [self.urls[0]]) + "\n", encoding="utf-8"
        )
        failures = self.audit()
        self.assertTrue(any("sitemap.txt: duplicate URL" in error
                            for error in failures))
        self.assertFalse(any("sitemap.xml:" in error for error in failures))

    def test_xml_text_and_current_content_must_all_agree(self) -> None:
        (self.site / "sitemap.txt").write_text(
            "\n".join(self.urls[:-1]) + "\n", encoding="utf-8"
        )
        failures = self.audit()
        self.assertIn("sitemap.txt: URL set differs from sitemap.xml", failures)
        self.assertTrue(any("sitemap.txt: missing current content URL" in error
                            for error in failures))
        # Agreement between two incomplete lists cannot stand in for the site.
        self.write(self.urls[:-1])
        failures = self.audit()
        for label in ("sitemap.xml", "sitemap.txt"):
            self.assertTrue(any(label + ": missing current content URL" in error
                                for error in failures))

    def test_bad_text_urls_cannot_hide_behind_valid_xml(self) -> None:
        for location in (
            "https://example.com/cohesix/docs/GUIDE.html",
            "https://lukeb-aidev.github.io/docs/GUIDE.html",
            CANONICAL_ROOT + "/docs/GUIDE.md",
            CANONICAL_ROOT + "/docs/REMOVED.html",
            CANONICAL_ROOT + "/index.html",
            CANONICAL_ROOT + "/404.html",
            CANONICAL_ROOT + "/docs/GUIDE.html#heading",
            "https://[invalid",
        ):
            with self.subTest(location=location):
                (self.site / "sitemap.txt").write_text(
                    "\n".join(self.urls + [location]) + "\n", encoding="utf-8"
                )
                failures = self.audit()
                self.assertTrue(any(error.startswith("sitemap.txt: URL")
                                    for error in failures))
                self.assertFalse(any("sitemap.xml:" in error for error in failures))

    def test_text_sitemap_accepts_crlf_and_an_unterminated_last_url(self) -> None:
        for ending in ("\n", "\r\n"):
            for final_newline in (False, True):
                with self.subTest(ending=ending, final_newline=final_newline):
                    text = ending.join(self.urls) + (ending if final_newline else "")
                    (self.site / "sitemap.txt").write_bytes(text.encode("utf-8"))
                    self.assertEqual(self.audit(), [])

    def test_missing_content_and_duplicate_urls_fail(self) -> None:
        self.write(self.urls[:-1])
        self.assertTrue(any("missing current content URL" in error
                            for error in self.audit()))
        self.write(self.urls + [self.urls[0]])
        self.assertTrue(any("duplicate URL" in error for error in self.audit()))

    def test_coverage_changes_with_the_actual_publication(self) -> None:
        added = self.site / "docs/NEW.html"
        added.write_text("<html>New content</html>", encoding="utf-8")
        self.assertTrue(any("missing current content URL" in error
                            and "NEW.html" in error for error in self.audit()))
        self.write(self.urls + [CANONICAL_ROOT + "/docs/NEW.html"])
        self.assertEqual(self.audit(), [])
        added.unlink()
        self.assertTrue(any("not current canonical content" in error
                            and "NEW.html" in error for error in self.audit()))

    def test_noncanonical_removed_and_foreign_urls_fail(self) -> None:
        for location in (
            "https://example.com/cohesix/docs/GUIDE.html",
            "https://lukeb-aidev.github.io/docs/GUIDE.html",
            "/cohesix/docs/GUIDE.html",
            CANONICAL_ROOT + "/docs/GUIDE.md",
            CANONICAL_ROOT + "/docs/REMOVED.html",
            CANONICAL_ROOT + "/404.html",
            CANONICAL_ROOT + "/google1234abcd.html",
            CANONICAL_ROOT + "/index.html",
            CANONICAL_ROOT + "/examples/index.html",
            CANONICAL_ROOT + "/docs/GUIDE.html?raw=1",
            CANONICAL_ROOT + "/docs/GUIDE.html#heading",
        ):
            with self.subTest(location=location):
                self.write(self.urls + [location])
                self.assertTrue(any("sitemap.xml: URL" in error
                                    for error in self.audit()))

    def test_missing_or_malformed_xml_and_bad_namespace_fail(self) -> None:
        target = self.site / "sitemap.xml"
        target.unlink()
        self.assertTrue(any("cannot read the generated sitemap" in error
                            for error in self.audit()))
        for document in (
            "<urlset>",
            "<urlset/>",
            '<urlset xmlns="https://wrong.example/sitemap"/>',
        ):
            with self.subTest(document=document):
                target.write_text(document, encoding="utf-8")
                self.assertTrue(self.audit())

    def test_each_namespaced_url_requires_exactly_one_text_location(self) -> None:
        for entry in (
            "<url/>", "<url><loc/></url>",
            "<url><loc>one</loc><loc>two</loc></url>",
            '<url><loc xmlns="">one</loc></url>',
            "<url><loc><nested>one</nested></loc></url>",
            "<unexpected/>",
        ):
            with self.subTest(entry=entry):
                document = sitemap_xml(self.urls).replace(
                    "</urlset>", entry + "</urlset>"
                )
                (self.site / "sitemap.xml").write_text(document, encoding="utf-8")
                self.assertTrue(any("exactly one text loc" in error
                                    or "only sitemap url elements" in error
                                    for error in self.audit()))

    def test_origin_and_baseurl_come_from_the_jekyll_configuration(self) -> None:
        (self.source / "_config.yml").write_text(
            "url: https://docs.example.org/\nbaseurl: /edge/\n", encoding="utf-8"
        )
        self.write([url.replace(CANONICAL_ROOT, "https://docs.example.org/edge")
                    for url in self.urls])
        self.assertEqual(self.audit(), [])
        (self.source / "_config.yml").write_text(
            'url: "https://docs.example.org"\nbaseurl: ""\n', encoding="utf-8"
        )
        self.write([url.replace(CANONICAL_ROOT, "https://docs.example.org")
                    for url in self.urls])
        self.assertEqual(self.audit(), [])

    def test_invalid_url_configuration_fails_explicitly(self) -> None:
        for config in (
            "url: https://example.com/path\n",
            "url: https://user:password@example.com\n",
            "url: https://example.com\nurl: https://example.org\n",
            "baseurl: ../outside\n",
            "baseurl: /edge/%2e%2e/outside\n",
            "baseurl: /edge?query=true\n",
            "baseurl: /edge//double\n",
        ):
            with self.subTest(config=config):
                (self.source / "_config.yml").write_text(config, encoding="utf-8")
                self.assertTrue(any(error.startswith("_config.yml:")
                                    for error in self.audit()))


if __name__ == "__main__":
    unittest.main()
