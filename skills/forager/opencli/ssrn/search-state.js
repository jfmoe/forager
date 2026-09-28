import { randomUUID } from 'node:crypto';

// OpenCLI reuses an already loaded URL; a nonce forces a fresh document.
// A new document keeps React's previous results out of this search.
export async function openSearch(page, kwargs) {
  const params = new URLSearchParams({
    term: kwargs.query, page: String(kwargs.page), text_fields: kwargs.scope,
    search_mode: kwargs.mode, authors: kwargs.author, date: kwargs.date, sort_by: kwargs.sort,
    _forager_search: randomUUID(),
  });
  await page.goto(`https://papers.ssrn.com/searchresults.cfm?${params}`, { waitUntil: 'load', settleMs: 500 });
}

export function readSearchState(kwargs) {
  const expected = JSON.stringify({
    text: kwargs.query, page: String(kwargs.page), text_fields: kwargs.scope,
    search_mode: kwargs.mode, authors: kwargs.author, date: kwargs.date, sort_by: kwargs.sort,
  });
  return `
    const expected = ${expected};
    if (new URL(location.href).searchParams.has('_forager_search')) return { state: 'pending', data: {} };
    const completed = performance.getEntriesByType('resource').filter((entry) => {
      const url = new URL(entry.name);
      return url.origin === 'https://api.ssrn.com'
        && url.pathname === '/papers/v1/papers/search/advanced' && entry.responseEnd > 0;
    }).at(-1);
    if (!completed) return { state: 'pending', data: {} };
    const requested = new URL(completed.name).searchParams;
    if (!Object.entries(expected).every(([key, value]) => requested.get(key) === value)) {
      return { state: 'pending', data: {} };
    }
    const value = (selector) => document.querySelector(selector)?.value ?? null;
    const searchState = {
      scope: document.querySelector('input[name="searchScope"]:checked')?.id ?? null,
      mode: document.querySelector('input[name="searchMode"]:checked')?.id ?? null,
      author: value('#author-input'), date: value('#trigger-input-date-range-dropdown'),
      sort: value('#maincontent input[role="combobox"]'), request_url: completed.name,
    };
  `;
}
