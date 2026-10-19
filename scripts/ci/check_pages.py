#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check generated Pages navigation, assets and Mermaid source.
# Copyright 2026 Lukas Bower

"""Check local Jekyll output without claiming browser or Mermaid execution."""

from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
from typing import NamedTuple
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
REQUIRED_ASSETS = {
    ("stylesheet", "apps/swarmui/frontend/styles/fonts.css"),
    ("stylesheet", "assets/css/site.css"),
    ("script", "assets/js/site.js"),
    ("icon", "apps/swarmui/frontend/assets/icons/cohesix-icon.svg"),
}
FONT_FILES = (
    "Inter-Variable.woff2",
    "JetBrainsMono-Variable.woff2",
    "INTER_LICENSE.txt",
    "JETBRAINS_MONO_OFL.txt",
)
VOID_TAGS = {
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link",
    "meta", "param", "source", "track", "wbr",
}
FENCE = re.compile(r"^([ \t]*)(`{3,}|~{3,})(.*)$")
CSS_URL = re.compile(r"url\(\s*['\"]?([^)'\"\s]+)['\"]?\s*\)")
JS_IMPORT = re.compile(
    r"^[ \t]*(?:import\s+(?:[^;'\"]*?\s+from\s+)?"
    r"|export\s+[^;'\"]*?\s+from\s+)['\"]([^'\"]*)['\"]",
    re.MULTILINE,
)


class CheckResult(NamedTuple):
    """Summarize static checks while retaining all actionable failures."""

    errors: list[str]
    pages: int
    diagrams: int


class Page(HTMLParser):
    """Collect shared chrome and decoded diagram text from generated HTML."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.stack: list[tuple[str, dict[str, str | None]]] = []
        self.ids: set[str] = set()
        self.markers: set[tuple[str, str]] = set()
        self.assets: list[tuple[str, str]] = []
        self.module_scripts: set[str] = set()
        self.chrome_links: list[str] = []
        self.navigation: list[str] = []
        self.source_links: list[str] = []
        self.diagrams: list[str] = []
        self.title = ""
        self.title_count = 0
        self.first_heading = ""
        self._found_heading = False
        self._title_depth: int | None = None
        self._heading_depth: int | None = None
        self._diagram_depth: int | None = None

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        values = dict(attrs)
        classes = (values.get("class") or "").split()
        identifier = values.get("id")
        if identifier:
            self.ids.add(identifier)
            self.markers.add((tag, identifier))
        for name in classes:
            self.markers.add((tag, name))
        in_article = any(
            v.get("id") == "document-content" for _, v in self.stack
        )
        if tag == "title" and any(t == "head" for t, _ in self.stack):
            self.title_count += 1
            self._title_depth = len(self.stack)
        if tag == "h1" and in_article and not self._found_heading:
            self._found_heading = True
            self._heading_depth = len(self.stack)
        if tag == "a":
            href = values.get("href") or ""
            if not in_article:
                self.chrome_links.append(href)
            if any(v.get("id") == "site-nav" for _, v in self.stack):
                self.navigation.append(href)
            if "source-link" in classes:
                self.source_links.append(href)
        if tag == "link":
            for relation in (values.get("rel") or "").split():
                if relation in {"stylesheet", "icon"}:
                    self.assets.append((relation, values.get("href") or ""))
        elif tag == "script" and "src" in values:
            src = values.get("src") or ""
            self.assets.append(("script", src))
            if values.get("type") == "module":
                self.module_scripts.add(src)
        elif tag == "img" and not in_article:
            self.assets.append(("image", values.get("src") or ""))
        if (
            tag == "code" and "language-mermaid" in classes
            and in_article and self.stack and self.stack[-1][0] == "pre"
        ):
            self.diagrams.append("")
            self._diagram_depth = len(self.stack)
        if tag not in VOID_TAGS:
            self.stack.append((tag, values))

    def handle_endtag(self, tag: str) -> None:
        for index in range(len(self.stack) - 1, -1, -1):
            if self.stack[index][0] == tag:
                if (
                    self._diagram_depth is not None
                    and index <= self._diagram_depth
                ):
                    self._diagram_depth = None
                if (
                    self._title_depth is not None
                    and index <= self._title_depth
                ):
                    self._title_depth = None
                if (
                    self._heading_depth is not None
                    and index <= self._heading_depth
                ):
                    self._heading_depth = None
                del self.stack[index:]
                break

    def handle_data(self, data: str) -> None:
        if self._diagram_depth is not None:
            self.diagrams[-1] += data
        if self._title_depth is not None:
            self.title += data
        if self._heading_depth is not None:
            self.first_heading += data


def mermaid_sources(path: Path) -> list[str]:
    """Extract Mermaid fences outside other fenced code examples."""
    marker = ""
    language = ""
    body: list[str] = []
    diagrams: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        fence = FENCE.match(line)
        if not marker:
            if fence:
                marker = fence[2]
                language = fence[3].strip()
                body = []
            continue
        if (
            fence and fence[2][0] == marker[0] and len(fence[2]) >= len(marker)
            and not fence[3].strip()
        ):
            if language == "mermaid":
                diagrams.append("\n".join(body) + "\n")
            marker = ""
        else:
            body.append(line)
    if marker:
        raise ValueError("unterminated Markdown code fence")
    return diagrams


def local_target(url: str, site: Path, baseurl: str) -> Path:
    """Resolve a project URL within the site, refusing path escapes."""
    parsed = urlsplit(url)
    if parsed.scheme or parsed.netloc:
        raise ValueError("expected a local project URL")
    path = unquote(parsed.path)
    if path != baseurl and not path.startswith(baseurl + "/"):
        raise ValueError(f"URL must start with {baseurl}/")
    remainder = path[len(baseurl):]
    relative = remainder.lstrip("/")
    if "//" in remainder or "\\" in relative or any(
        part in {".", ".."} for part in relative.split("/")
    ):
        raise ValueError("URL contains a traversal or ambiguous path")
    target = (site / relative).resolve()
    if not target.is_relative_to(site.resolve()):
        raise ValueError("URL escapes the generated site")
    if target.is_dir():
        target /= "index.html"
    if not target.is_file():
        raise ValueError("destination is missing")
    return target


def check_module_imports(
    scripts: set[Path], site: Path, baseurl: str
) -> list[str]:
    """Check the maintained scripts' literal static ESM dependency graph."""
    errors: list[str] = []
    pending = list(scripts)
    visited: set[Path] = set()
    while pending:
        script = pending.pop()
        if script in visited:
            continue
        visited.add(script)
        for specifier in JS_IMPORT.findall(script.read_text(encoding="utf-8")):
            parsed = urlsplit(specifier)
            if parsed.scheme or parsed.netloc:
                continue
            try:
                path = unquote(parsed.path)
                if path.startswith("/"):
                    target = local_target(specifier, site, baseurl)
                elif path.startswith(("./", "../")) and "\\" not in path:
                    target = (script.parent / path).resolve()
                    if not target.is_relative_to(site.resolve()):
                        raise ValueError("import escapes the generated site")
                    if not target.is_file():
                        raise ValueError("imported module is missing")
                else:
                    raise ValueError("import needs a local URL or import map")
                if target.suffix in {".js", ".mjs"}:
                    pending.append(target)
            except ValueError as error:
                errors.append(
                    f"{script.relative_to(site.resolve())}: "
                    f"local JS module {specifier!r}: {error}"
                )
    return errors


def check_site(
    site: Path, source: Path, baseurl: str = "/cohesix"
) -> CheckResult:
    """Validate public guides and the shared layout's local dependencies."""
    errors: list[str] = []
    sources = [(source / "README.md", Path("index.html"))]
    sources.extend((path, Path("docs") / path.with_suffix(".html").name)
                   for path in sorted((source / "docs").glob("*.md")))
    markers = {
        ("header", "site-header"), ("footer", "site-footer"),
        ("main", "main-content"), ("article", "document-content"),
        ("nav", "site-nav"), ("nav", "page-outline"),
    }
    reference_navigation: list[str] | None = None
    page_count = 0
    diagram_count = 0
    checked_assets: set[Path] = set()
    for markdown, relative in sources:
        label = relative.as_posix()
        generated = site / relative
        if not generated.is_file():
            errors.append(f"{label}: generated page is missing")
            continue
        page_count += 1
        page = Page()
        page.feed(generated.read_text(encoding="utf-8"))
        page.close()
        missing = sorted(markers - page.markers)
        if missing:
            errors.append(f"{label}: shared layout is missing {missing}")
        heading = " ".join(page.first_heading.split())
        title = " ".join(page.title.split())
        if (
            page.title_count != 1 or not heading
            or not (title == heading or title.startswith(heading + " · "))
        ):
            errors.append(
                f"{label}: browser title {title!r} must identify "
                f"the decoded first h1 {heading!r}"
            )
        if not page.navigation:
            errors.append(f"{label}: documentation navigation is empty")
        elif reference_navigation is None:
            reference_navigation = page.navigation
        elif page.navigation != reference_navigation:
            errors.append(
                f"{label}: documentation navigation differs from the homepage"
            )
        for href in page.chrome_links:
            parsed = urlsplit(href)
            if parsed.scheme in {"https", "http"} and parsed.netloc:
                continue
            if href.startswith("#"):
                if unquote(href[1:]) not in page.ids:
                    errors.append(f"{label}: missing chrome anchor {href!r}")
                continue
            try:
                local_target(href, site, baseurl)
            except ValueError as error:
                errors.append(f"{label}: chrome link {href!r}: {error}")
        for href in page.navigation:
            try:
                local_target(href, site, baseurl)
            except ValueError as error:
                errors.append(f"{label}: navigation {href!r}: {error}")
        source_path = markdown.relative_to(source).as_posix()
        expected_source = (
            "https://github.com/lukeb-aidev/cohesix/blob/main/" + source_path
        )
        if page.source_links != [expected_source]:
            errors.append(
                f"{label}: View source must link to {expected_source}"
            )
        found_assets: set[tuple[str, str]] = set()
        for kind, url in page.assets:
            try:
                asset = local_target(url, site, baseurl)
                checked_assets.add(asset)
                found_assets.add(
                    (kind, asset.relative_to(site.resolve()).as_posix())
                )
                if (
                    kind == "script" and asset.name == "site.js"
                    and url not in page.module_scripts
                ):
                    errors.append(
                        f"{label}: site.js must load with type=module"
                    )
            except ValueError as error:
                errors.append(f"{label}: {kind} asset {url!r}: {error}")
        if REQUIRED_ASSETS - found_assets:
            errors.append(
                f"{label}: missing theme assets "
                f"{sorted(REQUIRED_ASSETS - found_assets)}"
            )
        try:
            expected_diagrams = mermaid_sources(markdown)
        except (OSError, ValueError) as error:
            errors.append(f"{source_path}: {error}")
            continue
        diagram_count += len(expected_diagrams)
        if page.diagrams != expected_diagrams:
            errors.append(
                f"{label}: Mermaid source differs from {source_path} "
                f"({len(expected_diagrams)} expected, "
                f"{len(page.diagrams)} retained "
                "under pre > code.language-mermaid)"
            )
    for asset in sorted(checked_assets):
        if asset.suffix != ".css":
            continue
        for url in CSS_URL.findall(asset.read_text(encoding="utf-8")):
            target = (asset.parent / unquote(url)).resolve()
            if (
                not target.is_relative_to(site.resolve())
                or not target.is_file()
            ):
                errors.append(
                    f"{asset.relative_to(site.resolve())}: "
                    f"missing local CSS asset {url!r}"
                )
    errors.extend(check_module_imports(
        {path for path in checked_assets if path.suffix in {".js", ".mjs"}},
        site, baseurl,
    ))
    for filename in FONT_FILES:
        font = site / "apps/swarmui/frontend/assets/fonts" / filename
        if not font.is_file():
            errors.append(
                "native font asset or licence is missing: "
                f"{font.relative_to(site)}"
            )
    return CheckResult(errors, page_count, diagram_count)


def main() -> int:
    """Run the repository's static Pages publication checks."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--site", type=Path, required=True,
        help="generated Jekyll site directory",
    )
    parser.add_argument(
        "--source", type=Path, default=ROOT, help="Markdown source root"
    )
    arguments = parser.parse_args()
    result = check_site(arguments.site.resolve(), arguments.source.resolve())
    if result.errors:
        for error in result.errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(
        f"Pages static checks passed: {result.pages} pages, "
        f"{result.diagrams} Mermaid source blocks."
    )
    print(
        "Browser rendering, accessibility and public deployment "
        "remain separate checks."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
