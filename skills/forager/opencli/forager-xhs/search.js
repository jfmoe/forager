import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  PageFacts, SITE, SITE_DOMAIN, awaitCompletion, envelope, readDeadline, readExchanges,
} from './shared.js';

const SEARCH_PATH = '/api/sns/web/v2/search/notes';
const MAX_PAGES = 5;
const SCROLL_EVERY_MS = 4000;

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

const SCROLL_TO_END = `(() => {
  window.scrollTo(0, document.documentElement.scrollHeight);
})()`;

// A hidden background window renders no frame, so scrolling alone never loads the next page;
// a screenshot forces one.
async function scrollForMore(page) {
  await page.evaluate(SCROLL_TO_END);
  await page.screenshot();
  await page.screenshot();
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
    const deadline = readDeadline(kwargs.timeout);
    const pages = Math.min(MAX_PAGES, Math.max(1, Number(kwargs.pages) || 1));
    const facts = new PageFacts([SEARCH_PATH]);
    const responses = [];
    let filterClicks = 0;
    let filterFailure = null;
    let bodyMissing = false;
    let timedOut = false;

    // Keeps the search exchanges of a read, tagged with the filter clicks made so far. Only a
    // read after a completion signal expects every body; a request still in flight at a final
    // or deadline read is not one the command waited for.
    const keep = (exchanges, afterCompletion) => {
      for (const exchange of exchanges) {
        if (exchange.path !== SEARCH_PATH || exchange.method !== 'POST') continue;
        if (!exchange.has_body) {
          if (afterCompletion) bodyMissing = true;
          continue;
        }
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
    const completions = async () => {
      await facts.observe(page);
      return facts.completions(SEARCH_PATH);
    };
    // Waits for a search response after the `seen` completed ones; returns whether the command
    // can go on.
    const next = async (seen, nudge, orRendered = false) => {
      const result = await awaitCompletion(page, facts, {
        path: SEARCH_PATH,
        seen,
        deadline,
        nudge,
        nudgeEveryMs: SCROLL_EVERY_MS,
        orRendered,
      });
      keep(result.exchanges, result.state === 'completed');
      if (result.state === 'timed_out') timedOut = true;
      return result.state === 'completed' && !bodyMissing && !facts.ended();
    };
    const finish = async () => {
      const exchanges = await readExchanges(page);
      facts.absorb(exchanges);
      keep(exchanges, false);
      return envelope({
        page: facts.facts,
        filter_clicks: filterClicks,
        filter_failure: filterFailure,
        timed_out: timedOut,
        body_missing: bodyMissing,
        responses,
      });
    };

    await page.startNetworkCapture('xiaohongshu.com');
    const params = new URLSearchParams({ keyword: String(kwargs.query), source: 'web_explore_feed' });
    await page.goto(`https://${SITE_DOMAIN}/search_result?${params}`, { waitUntil: 'load', settleMs: 1000 });
    if (!await next(0, null, true)) return finish();

    for (const filter of FILTERS) {
      const value = String(kwargs[filter.arg] ?? filter.defaultValue);
      if (value === filter.defaultValue) continue;
      const option = filter.options[value];
      if (!option) {
        filterFailure = `unknown ${filter.arg} ${value}`;
        return finish();
      }
      const seen = await completions();
      const clicked = await page.evaluate(clickOptionScript(filter.group, option));
      if (clicked !== 'clicked') {
        filterFailure = `${clicked} for ${filter.group} ${option}`;
        return finish();
      }
      filterClicks += 1;
      if (!await next(seen, null)) return finish();
    }

    for (let read = 1; read < pages; read += 1) {
      const last = responses.at(-1);
      if (!last || !last.body.data || !last.body.data.has_more) break;
      if (!await next(await completions(), () => scrollForMore(page))) break;
    }
    return finish();
  },
});
