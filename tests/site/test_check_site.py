"""Tests for scripts/check_site.py. Run with: python -m unittest discover -s tests/site -p "test_*.py"."""

from __future__ import annotations

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

import check_site  # noqa: E402

GOOD = """<!doctype html><html lang="en"><head><title>Thing - REX Technologies</title>
<meta name="description" content="A thing."><link rel="stylesheet" href="styles.css">
<link rel="canonical" href="https://tochi-mba.github.io/lumap/"></head>
<body><a href="#top">top</a><main id="top"><h1>Thing</h1><img src="mark.svg" alt="">
<a href="https://github.com/tochi-mba/lumap">source</a><a href="other.html#part">other</a>
<a href="./">home</a></main><script src="app.js"></script></body></html>"""

OTHER = """<!doctype html><html lang="en"><head><title>Thing other</title>
<meta name="description" content="Other."></head><body><h1 id="part">REX Technologies</h1></body></html>"""

NOT_FOUND = """<!doctype html><html lang="en"><head><base href="/lumap/"><title>Thing</title>
<link rel="stylesheet" href="/lumap/styles.css"></head><body><h1>REX Technologies</h1></body></html>"""

SITEMAP = """<urlset><url><loc>https://tochi-mba.github.io/lumap/</loc></url>
<url><loc>https://tochi-mba.github.io/lumap/other.html</loc></url></urlset>"""


class CheckSiteTests(unittest.TestCase):
    def site(self, html: str = GOOD, nojekyll: bool = True, sitemap: str | None = SITEMAP) -> Path:
        root = Path(tempfile.mkdtemp())
        (root / "index.html").write_text(html, encoding="utf-8")
        (root / "other.html").write_text(OTHER, encoding="utf-8")
        (root / "404.html").write_text(NOT_FOUND, encoding="utf-8")
        for asset in ("styles.css", "app.js", "mark.svg"):
            (root / asset).write_text("", encoding="utf-8")
        if nojekyll:
            (root / ".nojekyll").write_text("", encoding="utf-8")
        if sitemap is not None:
            (root / "sitemap.xml").write_text(sitemap, encoding="utf-8")
        return root

    def index_problems(self, html: str) -> list[str]:
        return [p for p in check_site.check(self.site(html), "Thing") if p.startswith("index.html")]

    def test_the_shipped_site_passes(self) -> None:
        self.assertEqual([], check_site.check())

    def test_a_sound_site_has_no_problems(self) -> None:
        self.assertEqual([], check_site.check(self.site(), "Thing"))

    def test_a_missing_index_is_the_only_problem_worth_naming(self) -> None:
        empty = Path(tempfile.mkdtemp())
        self.assertEqual([f"{empty / 'index.html'} is missing; there is no site to publish."], check_site.check(empty, "Thing"))

    def test_each_kind_of_problem_is_named(self) -> None:
        cases = {
            "the title does not name Thing.": GOOD.replace("<title>Thing", "<title>Other"),
            "the page does not name REX Technologies.": GOOD.replace("REX Technologies", "REX"),
            "asset 'missing.css' is referenced but missing.": GOOD.replace("styles.css", "missing.css"),
            "anchor '#nowhere' points at an id that does not exist.": GOOD.replace('href="#top"', 'href="#nowhere"'),
            "image 'mark.svg' has no alt text.": GOOD.replace(' alt=""', ""),
            "draft text left in the page: 'coming soon'.": GOOD.replace("Thing</h1>", "Thing coming soon</h1>"),
            "the html element has no lang attribute.": GOOD.replace(' lang="en"', ""),
            "the page has 2 h1 headings; it needs exactly one.": GOOD.replace("<h1>Thing</h1>", "<h1>Thing</h1><h1>Again</h1>"),
            "the page has no meta description.": GOOD.replace('<meta name="description" content="A thing.">', ""),
            "ids are used more than once: ['top'].": GOOD.replace("<h1>", '<h1 id="top">'),
            "link 'gone.html' points at a page that does not exist.": GOOD.replace("other.html#part", "gone.html"),
            "link 'other.html#gone' points at an id that does not exist on other.html.": GOOD.replace("#part", "#gone"),
            "inline event handler 'main onclick' found; use app.js.": GOOD.replace('<main id="top">', '<main id="top" onclick="x()">'),
        }
        for message, html in cases.items():
            with self.subTest(message=message):
                self.assertEqual([f"index.html: {message}"], self.index_problems(html))

    def test_unsafe_new_tabs_insecure_links_and_foreign_owners_are_named(self) -> None:
        blank = GOOD.replace('<a href="./">', '<a href="https://github.com/tochi-mba/lumap" target="_blank">')
        self.assertIn("index.html: the new-tab link to 'https://github.com/tochi-mba/lumap' lacks rel=\"noopener\".", self.index_problems(blank))
        insecure = GOOD.replace('<a href="./">', '<a href="http://example.com/">')
        self.assertIn("index.html: links that are not https: ['http://example.com/'].", self.index_problems(insecure))
        foreign = GOOD.replace("github.com/tochi-mba/lumap", "github.com/someone/lumap")
        self.assertIn("index.html: the page links to GitHub owners other than tochi-mba: ['someone'].", self.index_problems(foreign))

    def test_a_not_found_page_needs_a_root_base(self) -> None:
        root = self.site()
        (root / "404.html").write_text(NOT_FOUND.replace('<base href="/lumap/">', ""), encoding="utf-8")
        problems = check_site.check(root, "Thing")
        self.assertEqual(1, len(problems))
        self.assertTrue(problems[0].startswith("404.html: the not-found page has no <base>"))

    def test_a_site_without_nojekyll_is_named(self) -> None:
        self.assertIn("site/.nojekyll", check_site.check(self.site(nojekyll=False), "Thing")[0])

    def test_the_sitemap_must_list_every_page_and_only_real_ones(self) -> None:
        self.assertEqual(["sitemap.xml is missing."], check_site.check(self.site(sitemap=None), "Thing"))
        partial = "<urlset><url><loc>https://tochi-mba.github.io/lumap/</loc></url><url><loc>https://tochi-mba.github.io/lumap/old.html</loc></url></urlset>"
        self.assertEqual(
            ["sitemap.xml does not list other.html.", "sitemap.xml lists old.html, which does not exist."],
            check_site.check(self.site(sitemap=partial), "Thing"),
        )

    def test_pages_starting_with_an_underscore_are_skipped(self) -> None:
        root = self.site()
        (root / "_draft.html").write_text("<html></html>", encoding="utf-8")
        self.assertEqual([], check_site.check(root, "Thing"))

    def test_remote_assets_and_empty_sources_are_not_checked_for_existence(self) -> None:
        html = GOOD.replace('<img src="mark.svg" alt="">', '<img src="" alt=""><img src="https://example.com/x.png" alt="">')
        self.assertEqual([], self.index_problems(html))

    def test_main_reports_and_exits_as_a_gate(self) -> None:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(0, check_site.main([str(self.site()), "Thing"]))
        self.assertIn("site checks passed", output.getvalue())
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(1, check_site.main([str(self.site(nojekyll=False)), "Thing"]))
        self.assertIn("1 problem(s) with the site:", output.getvalue())

    def test_main_defaults_to_the_shipped_site(self) -> None:
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(0, check_site.main([]))


if __name__ == "__main__":
    unittest.main()
