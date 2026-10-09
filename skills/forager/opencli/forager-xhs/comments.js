import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  ReadSession, SITE, SITE_DOMAIN, openNote, pageScript, pick, readDeadline,
} from './shared.js';

const PAGE_PATH = '/api/sns/web/v2/comment/page';
const SUB_PATH = '/api/sns/web/v2/comment/sub/page';
// Top-level comments in one `comment/page` response.
const PAGE_SIZE = 10;
const MAX_LIMIT = 50;
const MAX_EXPAND = 10;

// Whether the note page has finished its first comment request, from its own state.
function firstPageScript(id) {
  return pageScript(`
    const map = noteDetailMap();
    const entry = map && map[${JSON.stringify(id)}];
    const comments = entry && entry.comments;
    return unwrap(comments && comments.firstRequestFinish) === true;
  `);
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
    const id = String(kwargs.id);
    const limit = Math.min(MAX_LIMIT, Math.max(1, Number(kwargs.limit) || 1));
    const expand = Math.min(MAX_EXPAND, Math.max(0, Number(kwargs.expand) || 0));
    const responses = [];
    let expandFailure = null;

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
    const session = new ReadSession(page, {
      paths: [PAGE_PATH, SUB_PATH],
      deadline: readDeadline(kwargs.timeout),
      keep,
      fields: () => ({ expand_failure: expandFailure, responses }),
    });
    const pages = () => responses.filter((response) => response.kind === 'page');

    await page.startNetworkCapture('xiaohongshu.com');
    await openNote(page, id, kwargs['xsec-token']);
    const ready = () => page.evaluate(firstPageScript(id));
    if (!await session.next(PAGE_PATH, 0, { ready })) return session.finish();

    const needed = Math.ceil(limit / PAGE_SIZE);
    while (pages().length < needed) {
      const last = pages().at(-1);
      if (!last || !last.body.data || !last.body.data.has_more) break;
      if (!await session.more(PAGE_PATH)) return session.finish();
    }

    // Only comments among the first `limit` are delivered, so only they are expanded.
    const seenIds = new Set();
    const delivered = pages()
      .flatMap((response) => (response.body.data ? response.body.data.comments : []))
      .filter((item) => !seenIds.has(item.id) && seenIds.add(item.id))
      .slice(0, limit);
    const targets = delivered.filter((item) => item.sub_comment_has_more === true).slice(0, expand);
    for (const target of targets) {
      const seen = await session.completions(SUB_PATH);
      const clicked = await page.evaluate(expandScript(target.id));
      if (clicked !== 'clicked') {
        expandFailure = `${clicked} for comment ${target.id}`;
        return session.finish();
      }
      if (!await session.next(SUB_PATH, seen)) return session.finish();
    }
    return session.finish();
  },
});
