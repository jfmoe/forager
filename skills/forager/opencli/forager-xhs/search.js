import { cli, Strategy } from '@jackwener/opencli/registry';

import { ReadSession, SITE, SITE_DOMAIN, readDeadline } from './shared.js';

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

// Opens the filter panel the way a pointer does and clicks one option. Returns `clicked` or why
// it could not.
function clickOptionScript(group, option) {
  return `(async () => {
    const group = ${JSON.stringify(group)};
    const option = ${JSON.stringify(option)};
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const text = (element) => (element ? element.textContent : '').replace(/\\s+/g, '');
    const visible = (element) => {
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
    };
    const trigger = document.querySelector('.search-layout__top > .filter');
    if (!trigger) return 'no_filter_button';
    const panel = () => [...document.querySelectorAll('.search-layout__top > .filter > .filter-panel')].find(visible);
    for (let round = 0; round < 20 && !panel(); round += 1) {
      if (round === 0) {
        for (const type of ['pointerover', 'mouseover', 'mouseenter', 'mousemove']) {
          trigger.dispatchEvent(new MouseEvent(type, { bubbles: type !== 'mouseenter', view: window }));
        }
      }
      if (round === 5) trigger.click();
      await sleep(100);
    }
    const opened = panel();
    if (!opened) return 'no_filter_panel';
    const groups = [...opened.querySelectorAll('.filters')].filter((element) => {
      const label = [...element.children].find((child) => child.tagName === 'SPAN');
      return text(label) === group;
    });
    if (groups.length !== 1) return 'no_filter_group';
    const options = [...groups[0].querySelectorAll('.tags')].filter((element) => text(element) === option);
    if (options.length !== 1) return 'no_filter_option';
    options[0].click();
    return 'clicked';
  })()`;
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
      const clicked = await page.evaluate(clickOptionScript(filter.group, option));
      if (clicked !== 'clicked') {
        filterFailure = `${clicked} for ${filter.group} ${option}`;
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
