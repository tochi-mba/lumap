// Tests for site/app.js against a minimal stand-in document. Run with: node --test tests/site/app.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  RELEASE_API, RELEASES_PAGE, SUPPORTED, detectPlatform, applyPlatform, findInstaller, formatSize,
  describeRelease, fetchRelease, decorateDownload, wireMenu, wireCopyButtons, start,
} from '../../site/app.js';

class FakeElement {
  constructor(attributes = {}) {
    this.attributes = { ...attributes };
    this.listeners = {};
    this.textContent = attributes.text ?? '';
    this.dataset = attributes.dataset ?? {};
    this.classes = new Set();
    this.children = [];
    this.focused = false;
    this.classList = {
      toggle: (name, on) => (on ? this.classes.add(name) : this.classes.delete(name)),
      add: name => this.classes.add(name),
    };
  }

  getAttribute(name) { return this.attributes[name] ?? null; }

  setAttribute(name, value) { this.attributes[name] = value; }

  removeAttribute(name) { delete this.attributes[name]; }

  addEventListener(type, handler) { (this.listeners[type] ??= []).push(handler); }

  async fire(type, event = {}) { for (const handler of this.listeners[type] ?? []) await handler(event); }

  focus() { this.focused = true; }

  querySelectorAll() { return this.children; }
}

class FakeDocument extends FakeElement {
  constructor() {
    super();
    this.byId = {};
    this.bySelector = {};
  }

  getElementById(id) { return this.byId[id] ?? null; }

  querySelector(selector) { return (this.bySelector[selector] ?? [])[0] ?? null; }

  querySelectorAll(selector) { return this.bySelector[selector] ?? []; }
}

/** A document with the download button, its meta line and the unsupported note. */
function downloadDocument() {
  const doc = new FakeDocument();
  doc.byId.download = new FakeElement({ text: 'Download for Windows', href: 'static', download: 'x' });
  doc.byId['download-meta'] = new FakeElement({ text: 'Windows 10 and 11, 64-bit' });
  doc.byId.unsupported = new FakeElement();
  doc.bySelector['[data-latest-installer]'] = [doc.byId.download];
  return doc;
}

const release = {
  tag_name: 'v0.3.0',
  assets: [
    { name: 'lumap-0.3.0-windows-x64.zip', size: 1, browser_download_url: 'https://github.com/tochi-mba/lumap/releases/download/v0.3.0/lumap-0.3.0-windows-x64.zip' },
    { name: 'lumap-Setup-0.3.0.exe', size: 54_000_000, browser_download_url: 'https://github.com/tochi-mba/lumap/releases/download/v0.3.0/lumap-Setup-0.3.0.exe' },
  ],
};

const respond = (body, ok = true) => async () => ({ ok, json: async () => body });

test('the platform is read from the client hints first, then the user agent', () => {
  const cases = [
    [{ userAgentData: { platform: 'Windows' }, platform: 'MacIntel' }, 'windows'],
    [{ platform: 'Win32', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }, 'windows'],
    [{ platform: 'MacIntel', userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)' }, 'mac'],
    [{ userAgentData: { platform: 'macOS' } }, 'mac'],
    [{ platform: 'iPhone', userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)' }, 'mac'],
    [{ platform: 'Linux x86_64', userAgent: 'Mozilla/5.0 (X11; Linux x86_64)' }, 'linux'],
    [{ platform: 'Linux armv8l', userAgent: 'Mozilla/5.0 (Linux; Android 14)' }, 'linux'],
    [{ userAgentData: { platform: 'Chrome OS' }, userAgent: 'Mozilla/5.0 (X11; CrOS x86_64)' }, 'linux'],
    [{ platform: 'PlayStation', userAgent: 'Mozilla/5.0 (PlayStation 5)' }, 'other'],
    [{}, 'other'],
    [undefined, 'other'],
  ];
  for (const [navigator, expected] of cases) {
    assert.equal(detectPlatform(navigator), expected, JSON.stringify(navigator));
  }
});

test('on Windows the download button is left alone', () => {
  const doc = downloadDocument();
  assert.equal(applyPlatform(doc, 'windows'), false);
  assert.equal(doc.byId.download.textContent, 'Download for Windows');
  assert.equal(doc.byId.download.getAttribute('aria-disabled'), null);
  assert.ok(!doc.byId.unsupported.classes.has('shown'));
});

test('elsewhere the button says so, leads to the releases page and the note appears', () => {
  for (const [platform, name] of [['mac', 'macOS'], ['linux', 'Linux'], ['other', 'this device']]) {
    const doc = downloadDocument();
    assert.equal(applyPlatform(doc, platform), true);
    const button = doc.byId.download;
    assert.equal(button.textContent, 'Windows only for now');
    assert.equal(button.getAttribute('aria-disabled'), 'true');
    assert.equal(button.getAttribute('href'), RELEASES_PAGE);
    assert.equal(button.getAttribute('download'), null);
    assert.equal(doc.byId['download-meta'].textContent, `Not available for ${name} yet · Windows installer and portable zip on GitHub`);
    assert.ok(doc.byId.unsupported.classes.has('shown'));
  }
  assert.equal(applyPlatform(new FakeDocument(), 'mac'), true, 'a page without the elements is fine');
});

test('the installer is found by name and only when this repository hosts it', () => {
  assert.equal(findInstaller(release).name, 'lumap-Setup-0.3.0.exe');
  assert.equal(findInstaller({ assets: [{ name: 'lumap-Setup-1.exe', browser_download_url: 'https://evil.example/x.exe' }] }), null);
  assert.equal(findInstaller({}), null);
  assert.equal(findInstaller(null), null);
  assert.equal(findInstaller({ assets: [{}] }), null);
  assert.equal(findInstaller(release, 'mac'), null, 'no installer pattern for an unsupported platform');
  assert.deepEqual(Object.keys(SUPPORTED), ['windows']);
});

test('sizes and descriptions read naturally', () => {
  assert.equal(formatSize(54_000_000), '51 MB');
  assert.equal(formatSize(10), '1 MB');
  assert.equal(describeRelease(release, findInstaller(release)), 'v0.3.0 · 51 MB · Windows 10 and 11, 64-bit');
});

test('the release request asks GitHub once, without credentials, and survives failures', async () => {
  const calls = [];
  const fetcher = async (url, options) => { calls.push([url, options]); return { ok: true, json: async () => release }; };
  assert.equal((await fetchRelease(fetcher)).tag_name, 'v0.3.0');
  assert.equal(calls[0][0], RELEASE_API);
  assert.equal(calls[0][1].credentials, 'omit');
  assert.equal(await fetchRelease(respond(null, false)), null);
  assert.equal(await fetchRelease(async () => { throw new Error('offline'); }), null);
});

test('the download button points at the installer and the meta line describes it', async () => {
  const doc = downloadDocument();
  assert.equal(await decorateDownload(doc, respond(release)), true);
  const link = doc.byId.download;
  assert.equal(link.getAttribute('download'), 'lumap-Setup-0.3.0.exe');
  assert.match(link.getAttribute('href'), /lumap-Setup-0\.3\.0\.exe$/);
  assert.equal(doc.byId['download-meta'].textContent, 'v0.3.0 · 51 MB · Windows 10 and 11, 64-bit');
});

test('without a usable release, or on an unsupported platform, the static link stays', async () => {
  const doc = new FakeDocument();
  assert.equal(await decorateDownload(doc, respond({ assets: [] })), false);
  assert.equal(await decorateDownload(doc, respond(null, false)), false);
  let asked = 0;
  assert.equal(await decorateDownload(doc, async () => { asked += 1; return { ok: true, json: async () => release }; }, 'mac'), false);
  assert.equal(asked, 0, 'GitHub is not asked when there is nothing to offer');
  assert.equal(await decorateDownload(doc, respond(release)), true);
});

test('the menu opens, closes from a link, and Escape closes it only when open', async () => {
  const doc = new FakeDocument();
  const button = new FakeElement({ 'aria-expanded': 'false' });
  const nav = new FakeElement();
  const link = new FakeElement();
  nav.children = [link];
  doc.bySelector['.menu-button'] = [button];
  doc.bySelector['.site-nav'] = [nav];
  wireMenu(doc);

  await doc.fire('keydown', { key: 'Escape' });
  assert.equal(button.focused, false);
  await button.fire('click');
  assert.equal(button.getAttribute('aria-expanded'), 'true');
  assert.ok(nav.classes.has('open'));
  await doc.fire('keydown', { key: 'Enter' });
  assert.equal(button.getAttribute('aria-expanded'), 'true');
  await doc.fire('keydown', { key: 'Escape' });
  assert.equal(button.getAttribute('aria-expanded'), 'false');
  assert.equal(button.focused, true);
  await button.fire('click');
  await link.fire('click');
  assert.equal(button.getAttribute('aria-expanded'), 'false');
});

test('a page without a menu is left alone', () => {
  wireMenu(new FakeDocument());
});

test('copy buttons copy, announce and restore their label', async () => {
  const doc = new FakeDocument();
  const button = new FakeElement({ text: 'Copy', dataset: { copy: 'lumap new my-show.lumap.toml' } });
  const status = new FakeElement();
  doc.bySelector['.copy-button'] = [button];
  doc.byId['copy-status'] = status;
  const copied = [];
  const scheduled = [];
  wireCopyButtons(doc, { writeText: async text => { copied.push(text); } }, (fn, ms) => scheduled.push([fn, ms]));

  await button.fire('click');
  assert.deepEqual(copied, ['lumap new my-show.lumap.toml']);
  assert.equal(button.textContent, 'Copied');
  assert.equal(status.textContent, 'Command copied to the clipboard');
  scheduled[0][0]();
  assert.equal(button.textContent, 'Copy');
  assert.equal(scheduled[0][1], 1400);
});

test('a failed copy says so, with or without a status line', async () => {
  const doc = new FakeDocument();
  const button = new FakeElement({ text: 'Copy', dataset: { copy: 'x' } });
  doc.bySelector['.copy-button'] = [button];
  wireCopyButtons(doc, { writeText: async () => { throw new Error('denied'); } }, () => {});
  await button.fire('click');
  assert.equal(button.textContent, 'Copy failed');

  const announced = new FakeDocument();
  const second = new FakeElement({ text: 'Copy', dataset: { copy: 'x' } });
  announced.bySelector['.copy-button'] = [second];
  announced.byId['copy-status'] = new FakeElement();
  wireCopyButtons(announced, { writeText: async () => { throw new Error('denied'); } }, () => {});
  await second.fire('click');
  assert.equal(announced.byId['copy-status'].textContent, 'Could not copy the command');
});

test('start wires everything on Windows, fills the year and does nothing without a document', async () => {
  const doc = downloadDocument();
  doc.byId.year = new FakeElement();
  start(doc, { fetch: respond(release), navigator: { platform: 'Win32' }, setTimeout: () => {} });
  await new Promise(resolve => setImmediate(resolve));

  assert.equal(doc.byId.year.textContent, String(new Date().getFullYear()));
  assert.equal(doc.byId.download.getAttribute('download'), 'lumap-Setup-0.3.0.exe');
  assert.ok(!doc.byId.unsupported.classes.has('shown'));
  start(undefined, {});
  start(new FakeDocument(), {});
});

test('start on a Mac shows the not-yet state and never rewrites it with a release', async () => {
  const doc = downloadDocument();
  start(doc, { fetch: respond(release), navigator: { platform: 'MacIntel' }, setTimeout: () => {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(doc.byId.download.textContent, 'Windows only for now');
  assert.equal(doc.byId.download.getAttribute('href'), RELEASES_PAGE);
  assert.ok(doc.byId.unsupported.classes.has('shown'));
});

test('without a clipboard a copy reports failure', async () => {
  const doc = new FakeDocument();
  const button = new FakeElement({ text: 'Copy', dataset: { copy: 'x' } });
  doc.bySelector['.copy-button'] = [button];
  start(doc, {});
  await button.fire('click');
  assert.equal(button.textContent, 'Copy failed');
});
