import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  PageFacts, SITE, SITE_DOMAIN, awaitCompletion, envelope, readDeadline, readExchanges,
} from './shared.js';

const PAGE_PATH = '/api/sns/web/v2/comment/page';
const SUB_PATH = '/api/sns/web/v2/comment/sub/page';
// Top-level comments in one `comment/page` response.
const PAGE_SIZE = 10;
const MAX_LIMIT = 50;
const MAX_EXPAND = 10;
const SCROLL_EVERY_MS = 4000;

// Whether the note page has finished its first comment request, from its own state.
function firstPageScript(id) {
  return `(() => {
    const state = window.__INITIAL_STATE__;
    const map = state && state.note && state.note.noteDetailMap;
    const comments = map && map[${JSON.stringify(id)}] && map[${JSON.stringify(id)}].comments;
    let finished = comments && comments.firstRequestFinish;
    if (finished && typeof finished === 'object') finished = 'value' in finished ? finished.value : finished._value;
    return finished === true;
  })()`;
}

// The comment list scrolls inside the note panel, not the window.
const SCROLL_TO_END = `(() => {
  const scroller = document.querySelector('.note-scroller');
  if (scroller) scroller.scrollTop = scroller.scrollHeight;
  window.scrollTo(0, document.documentElement.scrollHeight);
})()`;

// A hidden background window renders no frame, so scrolling alone never loads the next page;
// a screenshot forces one.
async function scrollForMore(page) {
  await page.evaluate(SCROLL_TO_END);
  await page.screenshot();
  await page.screenshot();
}

// Clicks "展开 N 条回复" under one top-level comment, the way a user does. Returns `clicked` or
// why it could not.
function expandScript(id) {
  return `(async () => {
    const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const item = document.getElementById(${JSON.stringify(`comment-${id}`)});
    if (!item) return 'no_comment_element';
    const parent = item.closest('.parent-comment');
    if (!parent) return 'no_parent_comment';
    item.scrollIntoView({ block: 'center' });
    const find = () => [...parent.querySelectorAll('.reply-container .show-more')]
      .find((element) => /展开\\s*\\d+\\s*条回复/.test(element.textContent || ''));
    let button = find();
    for (let round = 0; round < 20 && !button; round += 1) {
      await sleep(100);
      button = find();
    }
    if (!button) return 'no_expand_button';
    button.click();
    return 'clicked';
  })()`;
}

function pick(object, keys) {
  const picked = {};
  for (const key of keys) if (object && object[key] !== undefined) picked[key] = object[key];
  return picked;
}

// Keeps the comment fields forager decodes; the users' own access tokens never leave the page.
function comment(raw) {
  return {
    ...pick(raw, [
      'id', 'content', 'create_time', 'ip_location', 'like_count', 'sub_comment_count',
      'sub_comment_cursor', 'sub_comment_has_more',
    ]),
    sub_comments: (raw.sub_comments || []).map(comment),
    user_info: pick(raw.user_info, ['user_id', 'nickname']),
    target_comment: raw.target_comment ? pick(raw.target_comment, ['id']) : null,
  };
}

cli({
  site: SITE,
  name: 'comments',
  access: 'read',
  description: 'Read the comments of one Xiaohongshu note, opened with its access token, for forager',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'id', required: true, valueRequired: true, help: 'Note ID' },
    { name: 'xsec-token', required: true, valueRequired: true, help: 'Access token of the note' },
    { name: 'limit', type: 'int', default: 20, help: 'Top-level comments to read, 1 to 50' },
    { name: 'expand', type: 'int', default: 0, help: 'Comments among the first `limit` whose replies to expand, 0 to 10' },
    { name: 'timeout', type: 'int', default: 120, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const id = String(kwargs.id);
    const limit = Math.min(MAX_LIMIT, Math.max(1, Number(kwargs.limit) || 1));
    const expand = Math.min(MAX_EXPAND, Math.max(0, Number(kwargs.expand) || 0));
    const facts = new PageFacts([PAGE_PATH, SUB_PATH]);
    const responses = [];
    let expandFailure = null;
    let bodyMissing = false;
    let timedOut = false;

    // Keeps the comment and reply responses of a read, with the request parameters that say
    // which note, page, and comment they answer.
    const keep = (exchanges) => {
      for (const exchange of exchanges) {
        const kind = { [PAGE_PATH]: 'page', [SUB_PATH]: 'sub' }[exchange.path];
        if (!kind || exchange.method !== 'GET' || !exchange.has_body) continue;
        const body = exchange.body || {};
        const data = body.data;
        responses.push({
          kind,
          params: pick(exchange.query, ['note_id', 'cursor', 'root_comment_id', 'num']),
          body: {
            msg: body.msg ?? null,
            data: data
              ? { cursor: data.cursor, has_more: data.has_more, comments: (data.comments || []).map(comment) }
              : null,
          },
        });
      }
    };
    const completions = async (path) => {
      await facts.observe(page);
      return facts.completions(path);
    };
    // Waits for a response to `path` after the `seen` completed ones; returns whether the
    // command can go on.
    const next = async (path, seen, nudge, ready = null) => {
      const result = await awaitCompletion(page, facts, {
        path,
        seen,
        deadline,
        nudge,
        nudgeEveryMs: SCROLL_EVERY_MS,
        ready,
      });
      keep(result.exchanges);
      if (result.state === 'body_missing') bodyMissing = true;
      if (result.state === 'timed_out') timedOut = true;
      return result.state === 'completed' && !facts.ended();
    };
    const finish = async () => {
      const exchanges = await readExchanges(page);
      facts.absorb(exchanges);
      keep(exchanges);
      return envelope({
        page: facts.facts,
        timed_out: timedOut,
        body_missing: bodyMissing,
        expand_failure: expandFailure,
        responses,
      });
    };
    const pages = () => responses.filter((response) => response.kind === 'page');

    await page.startNetworkCapture('xiaohongshu.com');
    const params = new URLSearchParams({ xsec_token: String(kwargs['xsec-token']), xsec_source: 'pc_search' });
    await page.goto(`https://${SITE_DOMAIN}/explore/${encodeURIComponent(id)}?${params}`, {
      waitUntil: 'load',
      settleMs: 1000,
    });
    if (!await next(PAGE_PATH, 0, null, () => page.evaluate(firstPageScript(id)))) return finish();

    const needed = Math.ceil(limit / PAGE_SIZE);
    while (pages().length < needed) {
      const last = pages().at(-1);
      if (!last || !last.body.data || !last.body.data.has_more) break;
      if (!await next(PAGE_PATH, await completions(PAGE_PATH), () => scrollForMore(page))) return finish();
    }

    // Only comments among the first `limit` are delivered, so only they are expanded.
    const seenIds = new Set();
    const delivered = pages()
      .flatMap((response) => (response.body.data ? response.body.data.comments : []))
      .filter((item) => !seenIds.has(item.id) && seenIds.add(item.id))
      .slice(0, limit);
    const targets = delivered.filter((item) => item.sub_comment_has_more === true).slice(0, expand);
    for (const target of targets) {
      const seen = await completions(SUB_PATH);
      const clicked = await page.evaluate(expandScript(target.id));
      if (clicked !== 'clicked') {
        expandFailure = `${clicked} for comment ${target.id}`;
        return finish();
      }
      if (!await next(SUB_PATH, seen, null)) return finish();
    }
    return finish();
  },
});
