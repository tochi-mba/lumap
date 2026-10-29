#!/usr/bin/env python3
"""Check the GitHub Pages site before it is published.

A static site has no compiler, so nothing otherwise catches a broken anchor, a missing asset, an
image a screen reader would read out by file name, or draft text that escaped. This is that gate;
the Pages workflow runs it on every push, and so does `cargo xtask ci`.

It is the REX family checker (title, company name, assets, anchors, alt text, draft words, GitHub
owner) with the stricter rules other REX sites added: a language, one h1, a description, https
only, unique ids, safe new-tab links, no inline handlers, a 404 page that works below the site
root, links between pages that resolve, and a sitemap that lists every page.

Standard library only, on purpose: the workflow should not install anything to verify a page
that has no build step.

Usage::

    python scripts/check_site.py                       # the site in this repository
    python scripts/check_site.py path/to/site lumap # any site, naming its product
"""

from __future__ import annotations

import re
import sys
from html.parser import HTMLParser
from pathlib import Path

SITE = Path(__file__).resolve().parents[1] / "site"
PRODUCT = "lumap"
OWNER = "tochi-mba"
"""Every GitHub link on a family site points at this owner; anything else is a stale copy."""

FORBIDDEN_PATTERNS = (
    r"\bTODO\b",
    r"\bFIXME\b",
    r"\bTBD\b",
    r"\bLorem ipsum\b",
    r"\bXXX\b",
    r"\bcoming soon\b",
)
"""Text that means a draft escaped."""

REMOTE = ("http://", "https://", "data:", "//", "mailto:")


class PageParser(HTMLParser):
    """Collects the ids, links and asset references a page depends on."""

    def __init__(self) -> None:
        super().__init__()
        self.ids: list[str] = []
        self.hrefs: list[str] = []
        self.assets: list[str] = []
        self.title = ""
        self.lang = ""
        self.description = ""
        self.base = ""
        self.h1_count = 0
        self.images_without_alt: list[str] = []
        self.unsafe_blank: list[str] = []
        self.inline_handlers: list[str] = []
        self._in_title = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = {key: (value or "") for key, value in attrs}
        if "id" in values:
            self.ids.append(values["id"])
        self.inline_handlers.extend(f"{tag} {key}" for key in values if key.startswith("on"))
        if tag == "html":
            self.lang = values.get("lang", "")
        if tag == "title":
            self._in_title = True
        if tag == "h1":
            self.h1_count += 1
        if tag == "base":
            self.base = values.get("href", "")
        if tag == "meta" and values.get("name") == "description":
            self.description = values.get("content", "")
        if tag == "a" and "href" in values:
            self.hrefs.append(values["href"])
            if values.get("target") == "_blank" and "noopener" not in values.get("rel", ""):
                self.unsafe_blank.append(values["href"])
        if tag == "img":
            self.assets.append(values.get("src", ""))
            # An explicitly empty alt marks an image decorative; a missing one does not.
            if "alt" not in values:
                self.images_without_alt.append(values.get("src") or "(no src)")
        if tag == "link" and "href" in values and values.get("rel") not in ("canonical", "preconnect"):
            self.assets.append(values["href"])
        if tag == "script" and values.get("src"):
            self.assets.append(values["src"])

    def handle_endtag(self, tag: str) -> None:
        if tag == "title":
            self._in_title = False

    def handle_data(self, data: str) -> None:
        if self._in_title:
            self.title += data


def check(site: Path = SITE, product: str = PRODUCT) -> list[str]:
    """Every problem with the site. Empty means it is publishable."""
    problems: list[str] = []
    if not (site / "index.html").exists():
        return [f"{site / 'index.html'} is missing; there is no site to publish."]
    if not (site / ".nojekyll").exists():
        problems.append(
            "site/.nojekyll is missing: without it Pages runs the site through Jekyll, "
            "which drops files beginning with an underscore."
        )
    pages = sorted(page for page in site.glob("*.html") if not page.name.startswith("_"))
    parsed = {page.name: _parse(page) for page in pages}
    for page in pages:
        problems.extend(f"{page.name}: {problem}" for problem in _check_page(page, parsed, product))
    problems.extend(_check_sitemap(site, pages))
    return problems


def _parse(page: Path) -> PageParser:
    parser = PageParser()
    parser.feed(page.read_text(encoding="utf-8"))
    return parser


def _check_page(page: Path, parsed: dict[str, PageParser], product: str) -> list[str]:
    problems: list[str] = []
    html = page.read_text(encoding="utf-8")
    parser = parsed[page.name]
    if product not in parser.title:
        problems.append(f"the title does not name {product}.")
    if "REX Technologies" not in html:
        problems.append("the page does not name REX Technologies.")
    if not parser.lang:
        problems.append("the html element has no lang attribute.")
    if parser.h1_count != 1:
        problems.append(f"the page has {parser.h1_count} h1 headings; it needs exactly one.")
    if page.name != "404.html" and not parser.description:
        problems.append("the page has no meta description.")
    if page.name == "404.html" and not parser.base.startswith("/"):
        problems.append(
            "the not-found page has no <base> at the site root: Pages serves it at the missing "
            "address, so its relative links and styles break below the top level."
        )
    duplicates = sorted({identifier for identifier in parser.ids if parser.ids.count(identifier) > 1})
    if duplicates:
        problems.append(f"ids are used more than once: {duplicates}.")
    for asset in parser.assets:
        if not asset or asset.startswith(REMOTE):
            continue
        if not _local(page, asset).exists():
            problems.append(f"asset '{asset}' is referenced but missing.")
    for href in parser.hrefs:
        problems.extend(_check_link(page, parser, parsed, href))
    problems.extend(f"image '{image}' has no alt text." for image in parser.images_without_alt)
    problems.extend(f"the new-tab link to '{href}' lacks rel=\"noopener\"." for href in parser.unsafe_blank)
    problems.extend(f"inline event handler '{handler}' found; use app.js." for handler in parser.inline_handlers)
    insecure = sorted(set(re.findall(r"http://[^\"'\s<>]+", html)))
    if insecure:
        problems.append(f"links that are not https: {insecure}.")
    for pattern in FORBIDDEN_PATTERNS:
        match = re.search(pattern, html, re.IGNORECASE)
        if match:
            problems.append(f"draft text left in the page: '{match.group(0)}'.")
    owners = {owner for owner in re.findall(r"https://github\.com/([^/\"'\s#?]+)", html) if owner}
    if owners - {OWNER}:
        problems.append(f"the page links to GitHub owners other than {OWNER}: {sorted(owners)}.")
    return problems


def _local(page: Path, reference: str) -> Path:
    """A reference resolved against the site folder. Every page lives at the site root, so a
    relative path and a root-absolute one ("/lumap/styles.css") name the same file."""
    path = reference.split("?")[0].split("#")[0]
    if path.startswith("/"):
        path = path.split("/", 2)[-1]
    return page.parent / path


def _check_link(page: Path, parser: PageParser, parsed: dict[str, PageParser], href: str) -> list[str]:
    if href.startswith(REMOTE) or not href or href == "#":
        return []
    if href.startswith("#"):
        return [] if href[1:] in parser.ids else [f"anchor '{href}' points at an id that does not exist."]
    target_name, _, fragment = href.partition("#")
    target = _local(page, target_name)
    if target.is_dir():
        target = target / "index.html"
    if not target.exists():
        return [f"link '{href}' points at a page that does not exist."]
    if fragment and target.name in parsed and fragment not in parsed[target.name].ids:
        return [f"link '{href}' points at an id that does not exist on {target.name}."]
    return []


def _check_sitemap(site: Path, pages: list[Path]) -> list[str]:
    sitemap = site / "sitemap.xml"
    if not sitemap.exists():
        return ["sitemap.xml is missing."]
    listed = set(re.findall(r"<loc>https://tochi-mba\.github\.io/lumap/([^<]*)</loc>", sitemap.read_text(encoding="utf-8")))
    expected = {"" if page.name == "index.html" else page.name for page in pages if page.name != "404.html"}
    problems = [f"sitemap.xml does not list {name or 'index.html'}." for name in sorted(expected - listed)]
    problems.extend(f"sitemap.xml lists {name}, which does not exist." for name in sorted(listed - expected))
    return problems


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    site = Path(args[0]) if args else SITE
    product = args[1] if len(args) > 1 else PRODUCT
    problems = check(site, product)
    if problems:
        print(f"{len(problems)} problem(s) with the site:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print("site checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
