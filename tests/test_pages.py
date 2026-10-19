# Author: Lukas Bower
# Purpose: Test static Pages publication failures with small site fixtures.
# Copyright 2026 Lukas Bower

"""Preserve layout and diagram-source contracts in generated documentation."""

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
        """Use the publication CLI's validation entry point."""
        return PAGES.check_site(self.site, self.source)

    def test_valid_nested_site_retains_decoded_source(self) -> None:
        result = self.audit()
        self.assertEqual(result.errors, [])
        self.assertEqual((result.pages, result.diagrams), (2, 1))

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


if __name__ == "__main__":
    unittest.main()
