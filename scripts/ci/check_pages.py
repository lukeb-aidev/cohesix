#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check Pages scope, content retention and canonical sitemap coverage.
# Copyright 2026 Lukas Bower

"""Check local Jekyll output without claiming browser or Mermaid execution."""

from __future__ import annotations

import argparse
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
from typing import NamedTuple
from urllib.parse import quote, unquote, urljoin, urlsplit, urlunsplit
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[2]
SOURCE_PREFIX = "https://github.com/lukeb-aidev/cohesix/blob/main/"
SITE_ORIGIN = "https://lukeb-aidev.github.io"
SITEMAP_NAMESPACE = "http://www.sitemaps.org/schemas/sitemap/0.9"
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
TABLE_SEPARATOR = re.compile(r":?-+:?")
CSS_URL = re.compile(r"url\(\s*['\"]?([^)'\"\s]+)['\"]?\s*\)")
JS_IMPORT = re.compile(
    r"^[ \t]*(?:import\s+(?:[^;'\"]*?\s+from\s+)?"
    r"|export\s+[^;'\"]*?\s+from\s+)['\"]([^'\"]*)['\"]",
    re.MULTILINE,
)
EXCLUDED_PATH = re.compile(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*/?")
ESCAPED_DISCLOSURE = re.compile(r"</\s*(?:summary|details)\s*>", re.IGNORECASE)
INLINE_CODE = re.compile(r"(?<!`)(`+)(?!`)[\s\S]*?(?<!`)\1(?!`)")
MARKDOWN_IMAGE = re.compile(
    r"(?<!\\)!\[[^\]\n]*\]\(\s*(?:<([^>\n]+)>|([^()\s]+))"
    r"(?:\s+(?:\"[^\"]*\"|'[^']*'))?\s*\)"
)


class CheckResult(NamedTuple):
    """Summarize static checks while retaining all actionable failures."""

    errors: list[str]
    pages: int
    diagrams: int


class CodeFence(NamedTuple):
    """Retain the source location, language and exact fenced content."""

    line: int
    language: str
    text: str


class Page(HTMLParser):
    """Collect shared chrome and decoded diagram text from generated HTML."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.stack: list[tuple[str, dict[str, str | None]]] = []
        self.ids: set[str] = set()
        self.markers: set[tuple[str, str]] = set()
        self.assets: list[tuple[str, str]] = []
        self.article_images: list[str] = []
        self.module_scripts: set[str] = set()
        self.chrome_links: list[str] = []
        self.navigation: list[str] = []
        self.source_links: list[str] = []
        self.diagrams: list[str] = []
        self.code_blocks: list[str] = []
        self.escaped_disclosures: list[str] = []
        self.tables = 0
        self.headed_tables = 0
        self.title = ""
        self.title_count = 0
        self.first_heading = ""
        self._found_heading = False
        self._title_depth: int | None = None
        self._heading_depth: int | None = None
        self._diagram_depth: int | None = None
        self._code_depth: int | None = None

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
        if tag == "table" and in_article:
            self.tables += 1
        if (
            tag == "thead" and in_article
            and any(t == "table" for t, _ in self.stack)
        ):
            self.headed_tables += 1
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
        elif tag == "img":
            if in_article:
                self.article_images.append(values.get("src") or "")
            else:
                self.assets.append(("image", values.get("src") or ""))
        if tag == "code" and in_article and self.stack:
            if self.stack[-1][0] == "pre":
                self.code_blocks.append("")
                self._code_depth = len(self.stack)
                if "language-mermaid" in classes:
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
                if self._code_depth is not None and index <= self._code_depth:
                    self._code_depth = None
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
        if self._code_depth is not None:
            self.code_blocks[-1] += data
        if self._diagram_depth is not None:
            self.diagrams[-1] += data
        if self._title_depth is not None:
            self.title += data
        if self._heading_depth is not None:
            self.first_heading += data
        if (
            any(v.get("id") == "document-content" for _, v in self.stack)
            and not any(t in {"pre", "code"} for t, _ in self.stack)
        ):
            self.escaped_disclosures.extend(ESCAPED_DISCLOSURE.findall(data))


def fenced_code_sources(path: Path) -> list[CodeFence]:
    """Extract fenced content without treating nested examples as fences."""
    marker = ""
    language = ""
    indentation = ""
    start = 0
    body: list[str] = []
    blocks: list[CodeFence] = []
    for number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        fence = FENCE.match(line)
        if not marker:
            if fence:
                marker = fence[2]
                language = fence[3].strip()
                indentation = fence[1]
                start = number
                body = []
            continue
        if (
            fence and fence[2][0] == marker[0] and len(fence[2]) >= len(marker)
            and not fence[3].strip()
        ):
            blocks.append(CodeFence(
                start, language, "\n".join(body) + ("\n" if body else "")
            ))
            marker = ""
        else:
            if "\t" not in indentation:
                remove = min(
                    len(indentation), len(line) - len(line.lstrip(" "))
                )
                line = line[remove:]
            elif line.startswith(indentation):
                line = line[len(indentation):]
            body.append(line)
    if marker:
        raise ValueError("unterminated Markdown code fence")
    return blocks


def mermaid_sources(path: Path) -> list[str]:
    """Extract Mermaid source using the same fenced-code boundary rules."""
    return [block.text for block in fenced_code_sources(path)
            if block.language == "mermaid"]


class SourceImages(HTMLParser):
    """Read raw HTML images and ordinary inline Markdown image references."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.images: list[str] = []
        self.code_depth = 0

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if tag in {"pre", "code"}:
            self.code_depth += 1
        elif tag == "img" and not self.code_depth:
            self.images.append(dict(attrs).get("src") or "")

    def handle_endtag(self, tag: str) -> None:
        if tag in {"pre", "code"} and self.code_depth:
            self.code_depth -= 1

    def handle_data(self, data: str) -> None:
        if self.code_depth:
            return
        for image in MARKDOWN_IMAGE.finditer(data):
            self.images.append(image[1] or image[2])
        if re.search(r"(?<!\\)!\[", MARKDOWN_IMAGE.sub("", data)):
            raise ValueError(
                "unsupported image syntax; use ordinary inline Markdown "
                "images or an HTML img element"
            )


def source_images(path: Path) -> list[str]:
    """Retain source images while excluding fenced and inline code examples."""
    marker = ""
    prose: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        fence = FENCE.match(line)
        if not marker:
            if fence:
                marker = fence[2]
            else:
                prose.append(line)
        elif (
            fence and fence[2][0] == marker[0] and len(fence[2]) >= len(marker)
            and not fence[3].strip()
        ):
            marker = ""
    if marker:
        raise ValueError("unterminated Markdown code fence")
    parser = SourceImages()
    parser.feed(INLINE_CODE.sub("", "\n".join(prose)))
    parser.close()
    return parser.images


def canonical_image_url(
    image: str, reference: Path, baseurl: str, *, source_relative: bool
) -> str:
    """Resolve local images against their source or output directory."""
    parsed = urlsplit(image)
    if parsed.scheme or parsed.netloc:
        return image
    if not parsed.path:
        raise ValueError("image src must name a file or external URL")
    if (
        source_relative and parsed.path.startswith("/")
        and not parsed.path.startswith(baseurl + "/")
    ):
        image = urlunsplit((
            "", "", baseurl + parsed.path, parsed.query, parsed.fragment
        ))
    base = baseurl + "/" + reference.parent.as_posix() + "/"
    resolved = urlsplit(urljoin(base, image))
    return urlunsplit((
        "", "", quote(unquote(resolved.path), safe="/"),
        resolved.query, resolved.fragment,
    ))


def publication_exclusions(source: Path) -> tuple[str, ...]:
    """Read the maintained literal exclude list without adding a YAML parser.

    Other configuration values are outside this check. Exclusions must use an
    indented list of unquoted repository paths; tags, aliases, globs and inline
    YAML collections fail explicitly rather than receiving guessed semantics.
    """
    config = source / "_config.yml"
    if not config.is_file():
        return ()
    exclusions: list[str] = []
    in_list = False
    seen = False
    header_line = 0
    indentation: int | None = None
    for number, line in enumerate(
        config.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if re.match(r"^exclude\s*:", line):
            if seen or not re.fullmatch(r"exclude:[ \t]*(?:#.*)?", line):
                raise ValueError(
                    f"line {number}: exclude must be one indented literal path list"
                )
            seen = in_list = True
            header_line = number
            continue
        if not in_list or not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line[0].isspace() and not line.startswith("-"):
            in_list = False
            continue
        item = re.fullmatch(r"( +)-[ ]+([^#]*?)[ ]*(?:[ ]+#.*)?", line)
        value = item[2].strip() if item else ""
        if item and indentation is None:
            indentation = len(item[1])
        if (
            not EXCLUDED_PATH.fullmatch(value)
            or (item is not None and len(item[1]) != indentation)
            or any(part in {".", ".."} for part in value.split("/"))
            or value.lower() in {"true", "false", "null"}
        ):
            raise ValueError(
                f"line {number}: exclude entries must be indented, unquoted "
                "literal repository paths without YAML tags or globs"
            )
        # Jekyll treats these as literal prefixes. A directory's trailing slash
        # protects archive siblings, so retain it until filesystem resolution.
        exclusions.append(value)
    if seen and not exclusions:
        raise ValueError(
            f"line {header_line}: exclude must contain an indented literal path list"
        )
    return tuple(exclusions)


def is_excluded(relative: Path, exclusions: tuple[str, ...]) -> bool:
    """Match Jekyll's literal prefixes, including meaningful trailing slashes."""
    return any(relative.as_posix().startswith(prefix) for prefix in exclusions)


def markdown_destination(relative: Path) -> Path:
    """Map the required Markdown routes used by the root Pages build."""
    return (Path("index.html") if relative == Path("README.md")
            else relative.with_suffix(".html"))


def excluded_outputs(site: Path, exclusions: tuple[str, ...]) -> list[str]:
    """Require excluded trees and directly mapped Markdown routes to be absent."""
    errors: list[str] = []
    for literal in exclusions:
        relative = Path(literal)
        outputs = {relative}
        if relative.suffix.lower() == ".md":
            outputs.add(markdown_destination(relative))
        for output in sorted(outputs):
            target = site / output
            if target.exists() or target.is_symlink():
                errors.append(
                    f"{output.as_posix()}: excluded output is still published "
                    f"(exclude: {literal})"
                )
    return errors


def pipe_cells(line: str) -> list[str]:
    """Split unescaped pipes, preserving backslash parity and optional edges."""
    cells: list[str] = []
    start = 0
    escaped = False
    for index, character in enumerate(line):
        if character == "|" and not escaped:
            cells.append(line[start:index].strip())
            start = index + 1
        escaped = not escaped if character == "\\" else False
    if not cells:
        return []
    cells.append(line[start:].strip())
    if not cells[0]:
        cells.pop(0)
    if cells and not cells[-1]:
        cells.pop()
    return cells


def pipe_table_lines(path: Path) -> list[int]:
    """Find ordinary pipe-table headers independently of block boundaries.

    A missing blank line must not hide a source table from this check just
    because a renderer swallowed it into another block. Fenced and indented
    code examples do not declare tables. Backslash-escaped pipes are content,
    rather than column boundaries, including within inline code.
    """
    marker = ""
    previous: list[str] = []
    in_table = False
    tables: list[int] = []
    for number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        fence = FENCE.match(line)
        if marker:
            if (
                fence and fence[2][0] == marker[0]
                and len(fence[2]) >= len(marker) and not fence[3].strip()
            ):
                marker = ""
            previous = []
            in_table = False
            continue
        if fence:
            marker = fence[2]
            previous = []
            in_table = False
            continue
        if line.startswith("\t") or len(line) - len(line.lstrip(" ")) > 3:
            previous = []
            in_table = False
            continue
        cells = pipe_cells(line.strip())
        if not cells:
            previous = []
            in_table = False
            continue
        if in_table:
            continue
        if (
            previous and len(previous) == len(cells)
            and all(TABLE_SEPARATOR.fullmatch(cell) for cell in cells)
        ):
            tables.append(number - 1)
            previous = []
            in_table = True
        else:
            previous = cells
    return tables


def check_table_preservation(
    site: Path, source: Path, parsed_pages: dict[Path, Page],
    published_sources: dict[Path, Path], exclusions: tuple[str, ...],
) -> list[str]:
    """Check table retention in published root guides and nested documents.

    Nested historical documents need not be published or adopt current chrome.
    When their HTML exists, however, it must retain their headed Markdown
    tables. Headerless Kramdown tables are outside the pipe-table contract.
    """
    errors: list[str] = []
    sources = [source / "README.md", source / "CONTRIBUTING.md"]
    sources.extend(sorted((source / "docs").rglob("*.md")))
    documents = {
        markdown_destination(path.relative_to(source)): path for path in sources
    }
    documents.update({generated: path for generated, path in published_sources.items()
                      if path.suffix.lower() == ".md"})
    for generated, markdown in sorted(documents.items()):
        relative = markdown.relative_to(source)
        if (
            is_excluded(relative, exclusions) or not markdown.is_file()
            or not (site / generated).is_file()
        ):
            continue
        expected = pipe_table_lines(markdown)
        page = parsed_pages.get(generated)
        if page is None:
            page = Page()
            page.feed((site / generated).read_text(encoding="utf-8"))
            page.close()
        if page.headed_tables != len(expected):
            errors.append(
                f"{generated.as_posix()}: table count differs from "
                f"{relative.as_posix()} ({len(expected)} pipe tables expected "
                f"at source lines {expected}, {page.headed_tables} retained "
                "with thead under article#document-content)"
            )
    return errors


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


def sitemap_location(source: Path, baseurl: str) -> tuple[str, str]:
    """Read the maintained literal URL scalars used by Jekyll absolute_url."""
    values = {"url": SITE_ORIGIN, "baseurl": baseurl}
    config = source / "_config.yml"
    seen: set[str] = set()
    if config.is_file():
        for number, line in enumerate(
            config.read_text(encoding="utf-8").splitlines(), start=1
        ):
            key = re.match(r"^(url|baseurl):", line)
            if not key:
                continue
            scalar = re.fullmatch(
                r"(url|baseurl):[ \t]*(\S+)(?:[ \t]+#.*)?", line
            )
            if not scalar or key[1] in seen:
                raise ValueError(
                    f"line {number}: {key[1]} must be one literal URL scalar"
                )
            value = scalar[2]
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            values[key[1]] = value
            seen.add(key[1])
    origin = urlsplit(values["url"])
    if (
        origin.scheme not in {"http", "https"} or not origin.hostname
        or origin.username is not None or origin.password is not None
        or origin.path not in {"", "/"} or origin.query or origin.fragment
    ):
        raise ValueError("url must be an absolute HTTP(S) origin")
    prefix = values["baseurl"]
    parsed = urlsplit(prefix)
    if (
        (prefix and not prefix.startswith("/")) or parsed.scheme or parsed.netloc
        or parsed.query or parsed.fragment or "//" in prefix or "\\" in prefix
        or any(part in {".", ".."} for part in unquote(prefix).split("/"))
    ):
        raise ValueError("baseurl must be an unambiguous project-root URL path")
    return values["url"].rstrip("/"), prefix.rstrip("/")


def sitemap_content_urls(site: Path, origin: str, baseurl: str) -> set[str]:
    """Derive canonical current content URLs from the actual HTML publication."""
    urls: set[str] = set()
    for generated in site.rglob("*.html"):
        relative = generated.relative_to(site)
        if relative == Path("404.html"):
            continue
        if (
            relative.parent == Path(".")
            and re.fullmatch(r"google[0-9a-f]+\.html", relative.name)
            and generated.read_text(encoding="utf-8").strip()
            == "google-site-verification: " + relative.name
        ):
            continue
        route = relative.as_posix()
        if relative.name == "index.html":
            route = "" if relative.parent == Path(".") else (
                relative.parent.as_posix() + "/"
            )
        urls.add(origin + baseurl + "/" + quote(route, safe="/"))
    return urls


def check_sitemap(site: Path, source: Path, baseurl: str) -> list[str]:
    """Require one canonical sitemap URL per current published HTML document."""
    errors: list[str] = []
    try:
        origin, prefix = sitemap_location(source, baseurl)
    except (OSError, ValueError) as error:
        return [f"_config.yml: {error}"]
    try:
        root = ElementTree.parse(site / "sitemap.xml").getroot()
    except (OSError, ElementTree.ParseError) as error:
        return [f"sitemap.xml: cannot read the generated sitemap: {error}"]
    namespace = "{" + SITEMAP_NAMESPACE + "}"
    if root.tag != namespace + "urlset":
        return ["sitemap.xml: root must be urlset in the standard sitemap namespace"]
    locations: list[str] = []
    for entry in root:
        if entry.tag != namespace + "url":
            errors.append("sitemap.xml: urlset may contain only sitemap url elements")
            continue
        nodes = entry.findall(namespace + "loc")
        if len(nodes) != 1 or len(nodes[0]) or not (nodes[0].text or "").strip():
            errors.append("sitemap.xml: each url must contain exactly one text loc")
            continue
        locations.append((nodes[0].text or "").strip())
    for location, count in Counter(locations).items():
        if count > 1:
            errors.append(f"sitemap.xml: duplicate URL {location!r}")
    expected = sitemap_content_urls(site, origin, prefix)
    expected_origin = urlsplit(origin)
    for location in sorted(set(locations)):
        parsed = urlsplit(location)
        if (
            parsed.scheme != expected_origin.scheme
            or parsed.netloc != expected_origin.netloc
            or not parsed.path.startswith(prefix + "/")
        ):
            errors.append(
                f"sitemap.xml: URL has a wrong origin or baseurl: {location!r}"
            )
        elif parsed.query or parsed.fragment or unquote(parsed.path).endswith(".md"):
            errors.append(f"sitemap.xml: URL is not canonical content: {location!r}")
        elif location not in expected:
            errors.append(
                f"sitemap.xml: URL is not current canonical content: {location!r}"
            )
    for location in sorted(expected - set(locations)):
        errors.append(f"sitemap.xml: missing current content URL {location!r}")
    return errors


def check_site(
    site: Path, source: Path, baseurl: str = "/cohesix"
) -> CheckResult:
    """Validate required guides and every source-linked published document."""
    errors: list[str] = []
    try:
        exclusions = publication_exclusions(source)
    except (OSError, ValueError) as error:
        return CheckResult([f"_config.yml: {error}"], 0, 0)
    errors.extend(excluded_outputs(site, exclusions))
    required = [source / "README.md"]
    required.extend(sorted((source / "docs").glob("*.md")))
    if (source / "404.html").is_file():
        required.append(source / "404.html")
    sources = {
        markdown_destination(path.relative_to(source)): path for path in required
        if not is_excluded(path.relative_to(source), exclusions)
    }
    markers = {
        ("header", "site-header"), ("footer", "site-footer"),
        ("main", "main-content"), ("article", "document-content"),
        ("nav", "site-nav"), ("nav", "page-outline"),
    }
    reference_navigation: list[str] | None = None
    page_count = 0
    diagram_count = 0
    checked_assets: set[Path] = set()
    parsed_pages: dict[Path, Page] = {}
    for generated in sorted(site.rglob("*.html")):
        relative = generated.relative_to(site)
        page = Page()
        page.feed(generated.read_text(encoding="utf-8"))
        page.close()
        parsed_pages[relative] = page
        for href in page.source_links:
            if not href.startswith(SOURCE_PREFIX):
                continue
            source_path = unquote(href[len(SOURCE_PREFIX):])
            path = Path(source_path)
            if path.suffix.lower() not in {".md", ".html"}:
                continue
            if (
                path.is_absolute() or "\\" in source_path
                or any(part in {".", ".."} for part in source_path.split("/"))
            ):
                errors.append(f"{relative}: View source has an unsafe repository path")
                continue
            if is_excluded(path, exclusions):
                errors.append(
                    f"{relative}: excluded source {source_path} is still published"
                )
                continue
            if not (source / path).is_file():
                errors.append(
                    f"{relative}: View source document {source_path} is missing"
                )
                continue
            # Required routes retain their expected source even if the link is wrong.
            sources.setdefault(relative, source / path)
    homepage = parsed_pages.get(Path("index.html"))
    if homepage and homepage.navigation:
        reference_navigation = homepage.navigation
    for relative, markdown in sorted(sources.items()):
        label = relative.as_posix()
        generated = site / relative
        if not generated.is_file():
            errors.append(f"{label}: generated page is missing")
            continue
        page_count += 1
        page = parsed_pages[relative]
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
        expected_source = SOURCE_PREFIX + source_path
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
        if page.escaped_disclosures:
            errors.append(
                f"{label}: escaped disclosure closing tags outside pre/code: "
                f"{page.escaped_disclosures}"
            )
        if markdown.suffix.lower() != ".md":
            continue
        try:
            fences = fenced_code_sources(markdown)
            expected_images = [
                canonical_image_url(image, Path(source_path), baseurl,
                                    source_relative=True)
                for image in source_images(markdown)
            ]
            retained_images = [
                canonical_image_url(image, relative, baseurl,
                                    source_relative=False)
                for image in page.article_images
            ]
        except (OSError, ValueError) as error:
            errors.append(f"{source_path}: {error}")
            continue
        if expected_images != retained_images:
            errors.append(
                f"{label}: article images differ from {source_path} "
                f"(expected {expected_images}, retained {retained_images})"
            )
        for image in dict.fromkeys(expected_images + retained_images):
            if urlsplit(image).scheme or urlsplit(image).netloc:
                continue
            try:
                local_target(image, site, baseurl)
            except ValueError as error:
                errors.append(f"{label}: article image {image!r}: {error}")
        expected_diagrams = [block.text for block in fences
                             if block.language == "mermaid"]
        diagram_count += len(expected_diagrams)
        if page.diagrams != expected_diagrams:
            errors.append(
                f"{label}: Mermaid source differs from {source_path} "
                f"({len(expected_diagrams)} expected, "
                f"{len(page.diagrams)} retained "
                "under pre > code.language-mermaid)"
            )
        cursor = 0
        for block in fences:
            try:
                cursor = page.code_blocks.index(block.text, cursor) + 1
            except ValueError:
                errors.append(
                    f"{label}: fenced code at {source_path}:{block.line} "
                    "is not retained exactly under article pre > code"
                )
    errors.extend(check_table_preservation(
        site, source, parsed_pages, sources, exclusions
    ))
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
    errors.extend(check_sitemap(site, source, baseurl))
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
