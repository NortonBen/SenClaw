// DOM-based search result extraction for content scripts.
// No Chrome API dependencies — self-contained for content script injection.
import type { SearchResultItem } from '../types/protocol';

export function extractGoogleResults(): SearchResultItem[] {
  const results: SearchResultItem[] = [];
  const containers = document.querySelectorAll('div.g, div[data-sokoban-container], div.MjjYud');

  containers.forEach((container, i) => {
    const link = container.querySelector('a[href]');
    const title = container.querySelector('h3');
    const snippet = container.querySelector('div.VwiC3b, span.aCOpRe, div[data-sncf], div[data-content-features]');

    if (link && title) {
      const href = link.getAttribute('href') ?? '';
      if (href.startsWith('http')) {
        results.push({
          position: i + 1,
          title: (title.textContent ?? '').trim(),
          url: href,
          snippet: (snippet?.textContent ?? '').trim(),
        });
      }
    }
  });

  return results;
}

export function extractBingResults(): SearchResultItem[] {
  const results: SearchResultItem[] = [];
  const containers = document.querySelectorAll('li.b_algo');

  containers.forEach((container, i) => {
    const link = container.querySelector('a[href]');
    const title = container.querySelector('h2');
    const snippet = container.querySelector('div.b_caption p, p.b_lineclamp2');

    if (link && title) {
      results.push({
        position: i + 1,
        title: (title.textContent ?? '').trim(),
        url: link.getAttribute('href') ?? '',
        snippet: (snippet?.textContent ?? '').trim(),
      });
    }
  });

  return results;
}
