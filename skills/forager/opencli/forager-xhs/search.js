import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  ReadSession, SITE, SITE_DOMAIN, clickTarget, confirmWithin, hoverTarget, inputFailureOf, pageScript, readDeadline,
} from './shared.js';

const SEARCH_PATH = '/api/sns/web/v2/search/notes';
const MAX_PAGES = 5;

// The filter panel's option text for each non-default option, in the order they are set.
const FILTERS = [
  {
    arg: 'sort', group: '排序依据', defaultValue: 'comprehensive',
    options: { latest: '最新', 'most-liked': '最多点赞', 'most-commented': '最多评论', 'most-collected': '最多收藏' },
  },
  { arg: 'note-type', group: '笔记类型', defaultValue: 'all', options: { image: '图文', video: '视频' } },
  {
    arg: 'publish-time', group: '发布时间', defaultValue: 'any',
    options: { day: '一天内', week: '一周内', 'half-year': '半年内' },
  },
];

// The panel opens on hover; a click on its trigger also opens it when the hover does not.
const PANEL_AFTER_HOVER_MS = 1000;
const PANEL_AFTER_CLICK_MS = 1500;
// How long a filter click has to show its effect: the option selected or a new results page.
const CLICK_CHECK_MS = 5000;

const TRIGGER = `document.querySelectorAll('.search-layout__top > .filter')`;

const PANEL_OPEN = pageScript(`
  return [...document.querySelectorAll('.search-layout__top > .filter > .filter-panel')].some((element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
  });
`);

// A page expression for the filter groups labelled `group`, in an open panel when `shown`.
function groupsExpression(group, shown) {
  return `(() => {
    const text = (element) => (element ? element.textContent : '').replace(/\\s+/g, '');
    return [...document.querySelectorAll('.search-layout__top > .filter > .filter-panel')]
      .filter((panel) => !${shown} || panel.getBoundingClientRect().height > 0)
      .flatMap((panel) => [...panel.querySelectorAll('.filters')])
      .filter((element) => text([...element.children].find((child) => child.tagName === 'SPAN')) === ${JSON.stringify(group)});
  })()`;
}

// A page expression for the options with the text `option` in the one shown group `group`.
function optionsExpression(group, option, shown) {
  return `(() => {
    const groups = ${groupsExpression(group, shown)};
    if (groups.length !== 1) return [];
    return [...groups[0].querySelectorAll('.tags')]
      .filter((element) => (element.textContent || '').replace(/\\s+/g, '') === ${JSON.stringify(option)});
  })()`;
}

function selectedScript(group, option) {
  return pageScript(`
    return [...${optionsExpression(group, option, false)}].some((element) => !element.closest('[aria-hidden="true"]')
      && (/\\b(active|selected|checked)\\b/.test(element.className)
        || ['aria-selected', 'aria-checked', 'aria-pressed'].some((name) => element.getAttribute(name) === 'true')));
  `);
}

// Opens the filter panel with the pointer and clicks one option, then confirms the click took
// effect. Returns null or why it could not.
async function chooseOption(page, session, group, option, seen) {
  const { deadline } = session;
  if (!await page.evaluate(PANEL_OPEN)) {
    const opening = { no_target: 'no_filter_button', ambiguous_target: 'ambiguous_filter_button' };
    const hovered = await inputFailureOf(() => hoverTarget(page, TRIGGER, { deadline }), opening);
    if (hovered) return hovered;
    if (!await confirmWithin(() => page.evaluate(PANEL_OPEN), PANEL_AFTER_HOVER_MS, deadline)) {
      const clicked = await inputFailureOf(() => clickTarget(page, TRIGGER, { deadline }), opening);
      if (clicked) return clicked;
      if (!await confirmWithin(() => page.evaluate(PANEL_OPEN), PANEL_AFTER_CLICK_MS, deadline)) {
        return 'no_filter_panel';
      }
    }
  }
  if (!await page.evaluate(pageScript(`return ${groupsExpression(group, true)}.length === 1;`))) {
    return 'no_filter_group';
  }
  const clicked = await inputFailureOf(
    () => clickTarget(page, optionsExpression(group, option, true), { deadline }),
    { no_target: 'no_filter_option', ambiguous_target: 'ambiguous_filter_option' },
  );
  if (clicked) return clicked;
  const landed = await confirmWithin(async () => session.facts.ended()
    || await session.completions(SEARCH_PATH) > seen
    || await page.evaluate(selectedScript(group, option)), CLICK_CHECK_MS, deadline);
  return landed ? null : 'click_missed';
}

cli({
  site: SITE,
  name: 'search',
  access: 'read',
  description: 'Read Xiaohongshu search result pages, with the requested filters, for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'query', required: true, valueRequired: true, help: 'Search words' },
    { name: 'sort', default: 'comprehensive', valueRequired: true, help: 'Result order' },
    { name: 'note-type', default: 'all', valueRequired: true, help: 'Note type' },
    { name: 'publish-time', default: 'any', valueRequired: true, help: 'Publish time' },
    { name: 'pages', type: 'int', default: 1, help: 'Result pages after the filters, 1 to 5' },
    { name: 'timeout', type: 'int', default: 120, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const pages = Math.min(MAX_PAGES, Math.max(1, Number(kwargs.pages) || 1));
    const responses = [];
    let filterClicks = 0;
    let filterFailure = null;

    // Keeps the search responses of a read, tagged with the filter clicks made so far.
    const keep = (exchanges) => {
      for (const exchange of exchanges) {
        if (exchange.path !== SEARCH_PATH || exchange.method !== 'POST' || !exchange.has_body) continue;
        const request = exchange.request || {};
        const body = exchange.body || {};
        responses.push({
          click: filterClicks,
          request: {
            keyword: request.keyword,
            page: request.page,
            search_id: request.search_id,
            filters: request.filters,
          },
          body: {
            msg: body.msg ?? null,
            data: body.data ? { has_more: body.data.has_more, items: body.data.items } : null,
          },
        });
      }
    };
    const session = new ReadSession(page, {
      paths: [SEARCH_PATH],
      deadline: readDeadline(kwargs.timeout),
      keep,
      fields: () => ({ filter_clicks: filterClicks, filter_failure: filterFailure, responses }),
    });

    await page.startNetworkCapture('xiaohongshu.com');
    const params = new URLSearchParams({ keyword: String(kwargs.query), source: 'web_explore_feed' });
    await page.goto(`https://${SITE_DOMAIN}/search_result?${params}`, { waitUntil: 'load', settleMs: 1000 });
    if (!await session.next(SEARCH_PATH, 0, { ready: () => session.facts.cards > 0 })) return session.finish();

    for (const filter of FILTERS) {
      const value = String(kwargs[filter.arg] ?? filter.defaultValue);
      if (value === filter.defaultValue) continue;
      const option = filter.options[value];
      if (!option) {
        filterFailure = `unknown ${filter.arg} ${value}`;
        return session.finish();
      }
      const seen = await session.completions(SEARCH_PATH);
      const failure = await chooseOption(page, session, filter.group, option, seen);
      if (failure) {
        filterFailure = `${failure} for ${filter.group} ${option}`;
        return session.finish();
      }
      filterClicks += 1;
      if (!await session.next(SEARCH_PATH, seen)) return session.finish();
    }

    for (let read = 1; read < pages; read += 1) {
      const last = responses.at(-1);
      if (!last || !last.body.data || !last.body.data.has_more) break;
      if (!await session.more(SEARCH_PATH)) break;
    }
    return session.finish();
  },
});
