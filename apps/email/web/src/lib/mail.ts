// Presentation helpers shared by the mail list, reading pane, and sidebar.

import type { Email } from '../api';

/** A message is unread when its IMAP flags array has no `\Seen` entry. */
export function isUnread(flags: string | null | undefined): boolean {
  if (!flags) return true;
  try {
    const arr: unknown = JSON.parse(flags);
    return Array.isArray(arr) ? !arr.includes('\\Seen') : true;
  } catch {
    return true;
  }
}

/** `Name <a@b.com>` → `Name`; a bare address falls back to its local part. */
export function displayName(from: string | null): string {
  if (!from) return '(Không rõ)';
  const quoted = from.match(/^\s*"?([^"<]+?)"?\s*<.+>/);
  if (quoted) return quoted[1].trim();
  const addr = from.replace(/[<>]/g, '').trim();
  return addr.split('@')[0] || addr;
}

/** `Name <a@b.com>` → `a@b.com`; a bare address is returned as-is. */
export function emailAddress(from: string | null): string {
  if (!from) return '';
  const angled = from.match(/<([^>]+)>/);
  return (angled ? angled[1] : from).trim();
}

const AVATAR_PALETTE = [
  '#2563eb', '#0891b2', '#7c3aed', '#db2777',
  '#ea580c', '#16a34a', '#ca8a04', '#dc2626',
];

/** Stable per-sender avatar colour. */
export function avatarColor(seed: string): string {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0;
  return AVATAR_PALETTE[h % AVATAR_PALETTE.length];
}

/**
 * List-column timestamp: time for today, day+month within this year, else
 * a full numeric date. Keeps the column narrow while staying unambiguous.
 */
export function formatListDate(ms: number | null): string {
  if (!ms) return '';
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return '';
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString('vi', { hour: '2-digit', minute: '2-digit' });
  }
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString('vi', { day: '2-digit', month: '2-digit' });
  }
  return d.toLocaleDateString('vi', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

/** Reading-pane timestamp: full date and time. */
export function formatFullDate(ms: number | null): string {
  if (!ms) return '';
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleString('vi', {
    weekday: 'short', day: '2-digit', month: '2-digit', year: 'numeric',
    hour: '2-digit', minute: '2-digit',
  });
}

/** `Re:`-prefix a subject without stacking duplicates. */
export function replySubject(subject: string | null): string {
  const s = (subject ?? '').trim();
  return /^re:/i.test(s) ? s : `Re: ${s}`;
}

export function unreadCount(emails: Email[]): number {
  return emails.filter(e => isUnread(e.flags)).length;
}

// ── HTML email sanitisation ────────────────────────────────────────────────
//
// Bodies come from untrusted senders, so the reading pane renders them inside a
// `sandbox=""` iframe — that alone blocks scripts, forms, and same-origin
// access. We still scrub the markup before it goes in, because the sandbox does
// not stop remote requests: a bare <img> is a tracking pixel that tells the
// sender the mail was opened. Images are therefore held back until the user
// asks for them, mirroring what Gmail and Apple Mail do.

const DANGEROUS_TAGS = ['script', 'iframe', 'object', 'embed', 'link', 'meta', 'base', 'form'];

/** Attributes carrying a URL that must be scheme-checked. */
const URL_ATTRS = ['href', 'src', 'action', 'background', 'poster'];

const SAFE_SCHEMES = /^(https?:|mailto:|tel:|cid:|data:image\/(png|jpeg|gif|webp);)/i;

export interface SanitizeResult {
  html: string;
  /** True when at least one remote image was withheld. */
  blockedImages: boolean;
}

/**
 * Strip active content from an HTML email body.
 *
 * @param showImages when false, remote image sources are parked in `data-src`
 *   so nothing is fetched until the user opts in.
 */
export function sanitizeEmailHtml(raw: string, showImages: boolean): SanitizeResult {
  const doc = new DOMParser().parseFromString(raw, 'text/html');
  let blockedImages = false;

  doc.querySelectorAll(DANGEROUS_TAGS.join(',')).forEach(el => el.remove());

  doc.querySelectorAll('*').forEach(el => {
    for (const attr of Array.from(el.attributes)) {
      const name = attr.name.toLowerCase();
      const value = attr.value.trim();

      // Inline event handlers (onclick, onerror, …) and javascript: URLs.
      if (name.startsWith('on')) {
        el.removeAttribute(attr.name);
        continue;
      }
      if (URL_ATTRS.includes(name) && value && !SAFE_SCHEMES.test(value)) {
        el.removeAttribute(attr.name);
        continue;
      }
      // `style` can pull remote urls (background-image) and position elements
      // outside the frame; drop the url() bits only.
      if (name === 'style' && /url\s*\(/i.test(value) && !showImages) {
        el.setAttribute('style', value.replace(/url\s*\([^)]*\)/gi, 'none'));
        blockedImages = true;
      }
    }
  });

  if (!showImages) {
    doc.querySelectorAll('img[src]').forEach(img => {
      const src = img.getAttribute('src') ?? '';
      // Embedded data: images are already local — no request, no tracking.
      if (/^data:/i.test(src)) return;
      img.setAttribute('data-src', src);
      img.removeAttribute('src');
      img.removeAttribute('srcset');
      blockedImages = true;
    });
  } else {
    doc.querySelectorAll('img[data-src]').forEach(img => {
      img.setAttribute('src', img.getAttribute('data-src') ?? '');
      img.removeAttribute('data-src');
    });
  }

  // Links must escape the sandboxed frame rather than navigate it.
  doc.querySelectorAll('a[href]').forEach(a => {
    a.setAttribute('target', '_blank');
    a.setAttribute('rel', 'noopener noreferrer nofollow');
  });

  return { html: doc.body.innerHTML, blockedImages };
}

/** Escape text so it can be embedded in the reading pane's HTML document. */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * Render a plain-text body as HTML: escape it, then turn bare URLs into links.
 * Trailing punctuation is left outside the link so `(see https://x.com)` works.
 */
export function textToHtml(text: string): string {
  const escaped = escapeHtml(text);
  return escaped.replace(
    /\bhttps?:\/\/[^\s<]+/g,
    (url) => {
      const trimmed = url.replace(/[.,;:!?)\]}>]+$/, '');
      const tail = url.slice(trimmed.length);
      return `<a href="${trimmed}" target="_blank" rel="noopener noreferrer nofollow">${trimmed}</a>${tail}`;
    },
  );
}
