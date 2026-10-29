// The site's behaviour: the mobile menu, copy buttons, the footer year, and the download button,
// which follows the visitor's operating system and the latest release. Pure functions are exported
// for the tests in tests/site; start() wires them to a document and does nothing without one, so
// importing this file in Node is harmless.

export const REPOSITORY = 'tochi-mba/lumap';
export const RELEASE_API = `https://api.github.com/repos/${REPOSITORY}/releases/latest`;
export const RELEASES_PAGE = `https://github.com/${REPOSITORY}/releases/latest`;
const DOWNLOAD_PREFIX = `https://github.com/${REPOSITORY}/releases/download/`;

/** The platforms lumap ships for. A platform missing here gets the "not yet" state. */
export const SUPPORTED = {
  windows: { label: 'Download for Windows', meta: 'Windows 10 and 11, 64-bit', installer: /^lumap-Setup-.*\.exe$/ },
};

export const PLATFORM_NAMES = { windows: 'Windows', mac: 'macOS', linux: 'Linux', other: 'this device' };

/** Which platform a browser runs on: windows, mac, linux or other. Never throws. */
export function detectPlatform(navigator) {
  const hint = String(navigator?.userAgentData?.platform ?? '');
  const platform = String(navigator?.platform ?? '');
  const agent = String(navigator?.userAgent ?? '');
  const text = `${hint} ${platform} ${agent}`;
  if (/windows|win32|win64/i.test(hint) || /^win/i.test(platform) || /windows nt/i.test(agent)) return 'windows';
  if (/mac|iphone|ipad|ipod/i.test(text)) return 'mac';
  if (/android|linux|cros|x11/i.test(text)) return 'linux';
  return 'other';
}

/**
 * Shapes the download button for a platform. Windows keeps the installer link; any other platform
 * gets an honest "not yet" button that still leads to the releases page, and the note is shown.
 */
export function applyPlatform(doc, platform) {
  if (SUPPORTED[platform]) return false;
  const name = PLATFORM_NAMES[platform] ?? PLATFORM_NAMES.other;
  const button = doc.getElementById('download');
  if (button) {
    button.textContent = 'Windows only for now';
    button.setAttribute('aria-disabled', 'true');
    button.setAttribute('href', RELEASES_PAGE);
    button.removeAttribute('download');
  }
  const meta = doc.getElementById('download-meta');
  if (meta) meta.textContent = `Not available for ${name} yet · Windows installer and portable zip on GitHub`;
  const note = doc.getElementById('unsupported');
  if (note) note.classList.add('shown');
  return true;
}

/** The installer asset of a release, accepted only when it is hosted by this repository. */
export function findInstaller(release, platform = 'windows') {
  const pattern = SUPPORTED[platform]?.installer;
  if (!pattern) return null;
  const assets = Array.isArray(release?.assets) ? release.assets : [];
  return assets.find(asset =>
    pattern.test(asset?.name ?? '') &&
    String(asset?.browser_download_url ?? '').startsWith(DOWNLOAD_PREFIX)) ?? null;
}

export function formatSize(bytes) {
  return `${Math.max(1, Math.round(bytes / 1048576))} MB`;
}

/** The line under the download button, such as "v0.3.0 · 52 MB · Windows 10 and 11, 64-bit". */
export function describeRelease(release, installer, platform = 'windows') {
  return `${release.tag_name} · ${formatSize(installer.size)} · ${SUPPORTED[platform].meta}`;
}

/** Asks GitHub for the latest release; any failure leaves the static link in place. */
export async function fetchRelease(fetcher) {
  try {
    const response = await fetcher(RELEASE_API, { headers: { Accept: 'application/vnd.github+json' }, credentials: 'omit' });
    return response.ok ? await response.json() : null;
  } catch {
    return null;
  }
}

/** Points every installer link at the newest release and describes it. False when nothing changed. */
export async function decorateDownload(doc, fetcher, platform = 'windows') {
  if (!SUPPORTED[platform]) return false;
  const release = await fetchRelease(fetcher);
  const installer = release ? findInstaller(release, platform) : null;
  if (!installer) return false;
  doc.querySelectorAll('[data-latest-installer]').forEach(link => {
    link.setAttribute('href', installer.browser_download_url);
    link.setAttribute('download', installer.name);
  });
  const meta = doc.getElementById('download-meta');
  if (meta) meta.textContent = describeRelease(release, installer, platform);
  return true;
}

export function wireMenu(doc) {
  const button = doc.querySelector('.menu-button');
  const nav = doc.querySelector('.site-nav');
  if (!button || !nav) return;
  const isOpen = () => button.getAttribute('aria-expanded') === 'true';
  const setMenu = (open, returnFocus = false) => {
    nav.classList.toggle('open', open);
    button.setAttribute('aria-expanded', String(open));
    if (!open && returnFocus) button.focus();
  };
  button.addEventListener('click', () => setMenu(!isOpen()));
  nav.querySelectorAll('a').forEach(link => link.addEventListener('click', () => setMenu(false)));
  // Escape closes an open menu and returns focus to its button; with the menu closed it does nothing.
  doc.addEventListener('keydown', event => {
    if (event.key === 'Escape' && isOpen()) setMenu(false, true);
  });
}

export function wireCopyButtons(doc, clipboard, schedule) {
  const status = doc.getElementById('copy-status');
  doc.querySelectorAll('.copy-button').forEach(button => button.addEventListener('click', async () => {
    const label = button.textContent;
    let done = true;
    try {
      await clipboard.writeText(button.dataset.copy);
    } catch {
      done = false;
    }
    button.textContent = done ? 'Copied' : 'Copy failed';
    if (status) status.textContent = done ? 'Command copied to the clipboard' : 'Could not copy the command';
    schedule(() => { button.textContent = label; }, 1400);
  }));
}

export function start(doc = globalThis.document, win = globalThis) {
  if (!doc) return;
  wireMenu(doc);
  wireCopyButtons(doc, win.navigator?.clipboard ?? { writeText: () => Promise.reject(new Error('no clipboard')) }, win.setTimeout?.bind(win) ?? (() => {}));
  const year = doc.getElementById('year');
  if (year) year.textContent = String(new Date().getFullYear());
  const platform = detectPlatform(win.navigator);
  applyPlatform(doc, platform);
  if (typeof win.fetch === 'function') decorateDownload(doc, win.fetch.bind(win), platform);
}

start();
