import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  HOME_URL, PageFacts, SITE, SITE_DOMAIN, envelope, navigate, openConversation, readDeadline,
  waitFor,
} from './shared.js';

// Labels in both interface languages. The zh-CN interface of 2026-10-10 showed a 撰写器模式
// switch with 聊天 and 工作, an 添加文件等内容 button whose chat-mode menu holds 深度研究, and a
// plan card with 编辑, 取消, and a counting-down 开始. The card lives in a cross-origin iframe
// from web-sandbox.oaiusercontent.com titled 深度研究, which OpenCLI can neither inspect nor
// click into, so the adapter never clicks 开始 and waits out the countdown instead. The English
// labels, the send button, and the reply and usage-notice detection are unverified; the live
// acceptance run confirms them.
const COMPOSER = '#prompt-textarea';
const MODE_SWITCH = ['撰写器模式', 'composer mode'];
const CHAT_MODE = ['聊天', 'chat'];
const TOOLS_BUTTON = ['添加文件等内容', 'add files and more'];
const DEEP_RESEARCH = ['深度研究', 'deep research'];
const SEND = ['发送提示', '发送', 'send prompt', 'send'];
const DEEP_RESEARCH_APP = 'connector_openai_deep_research';
const CARD_FRAME_DOMAIN = 'oaiusercontent.com';

// The usage wording is an unverified guess. It counts only after sending, outside the
// conversation turns and the navigation, and only as a line the page did not show before
// sending (a workspace banner about another member's limit can already show).
const QUOTA_PATTERN = '(reached|hit) (your|the) [^\\n]{0,40}limit|usage limit|已达到[^\\n]{0,20}(上限|限额)|已達到[^\\n]{0,20}(上限|限額)|额度已用(完|尽)|使用上限';
const NOT_A_NOTICE = [
  'nav', 'aside', '[role="navigation"]', '[data-message-author-role]', 'article',
  '[contenteditable="true"]', 'textarea',
].join(', ');

const TARGET_ATTRIBUTE = 'data-forager-target';
const MENU_WAIT_MS = 10000;
const SWITCH_WAIT_MS = 5000;
const MENTION_WAIT_MS = 5000;
const CONVERSATION_WAIT_MS = 30000;
// The plan card showed within 15 s live. A reply only counts once it stayed without a plan
// card this long, so a card still loading never reads as a reply.
const CARD_WAIT_MS = 60000;
const REPLY_STABLE_MS = 10000;
const CARD_POLL_MS = 2000;
// With the tab kept open, ChatGPT started the research by itself 1 s after
// `waiting_for_user_response_on_plan_until`, about 60 s after the plan card showed. The wait
// leaves 30 s on top for a card noticed late and for the start to reach the conversation, but
// always stops early enough to leave the reopen its time.
const COUNTDOWN_WAIT_MS = 90000;
const REOPEN_RESERVE_MS = 20000;

/** Page-side helpers shared by the scripts: visibility, labels, and targets. */
const HELPERS = `
  const isVisible = (el) => {
    if (!(el instanceof HTMLElement)) return false;
    if (el.closest('[hidden], [aria-hidden="true"]')) return false;
    const style = window.getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0) return false;
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  const isEnabled = (el) => !el.disabled && el.getAttribute('aria-disabled') !== 'true';
  const labelOf = (el) => ((el.getAttribute('aria-label') || '') + ' ' + (el.textContent || ''))
    .replace(/\\s+/g, ' ').trim().toLowerCase();
  // Earlier labels win over later ones, so a short fallback label never beats an exact one.
  const byLabels = (elements, labels) => {
    for (const label of labels) {
      const found = elements.find((el) => labelOf(el).includes(label));
      if (found) return found;
    }
    return null;
  };
  const clickables = (root) => Array.from(root.querySelectorAll(
    'button, [role="button"], [role="radio"], [role="tab"], [role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"], [role="option"]',
  )).filter((el) => isVisible(el) && isEnabled(el));
  const isSelected = (el) => ['aria-checked', 'aria-selected', 'aria-pressed']
    .some((name) => el.getAttribute(name) === 'true')
    || ['on', 'active', 'checked'].includes(el.getAttribute('data-state') || '');
  const composer = () => Array.from(document.querySelectorAll(
    '${COMPOSER}, [contenteditable="true"][role="textbox"]',
  )).find(isVisible);
  // Marks one element so that OpenCLI's click primitive can find it by selector.
  const mark = (el, name) => {
    for (const old of document.querySelectorAll('[${TARGET_ATTRIBUTE}="' + name + '"]')) old.removeAttribute('${TARGET_ATTRIBUTE}');
    if (!el) return false;
    el.setAttribute('${TARGET_ATTRIBUTE}', name);
    return true;
  };
`;

function script(body, ...values) {
  return `((${values.map((_, index) => `arg${index}`).join(', ')}) => {${HELPERS}${body}})(${values.map((value) => JSON.stringify(value)).join(', ')})`;
}

function target(name) {
  return `[${TARGET_ATTRIBUTE}="${name}"]`;
}

/**
 * The composer mode: `chat`; `switch` after marking the chat option to click; `missing` when the
 * mode switch has no chat option; `none` when the page shows no mode switch, as for an account
 * without the work mode.
 */
const composerMode = script(`
  const group = Array.from(document.querySelectorAll('[aria-label]'))
    .filter(isVisible)
    .find((el) => arg0.some((label) => (el.getAttribute('aria-label') || '').toLowerCase().includes(label)));
  if (!group) return 'none';
  const chat = byLabels(clickables(group), arg1);
  if (!chat) return 'missing';
  if (isSelected(chat)) return 'chat';
  mark(chat, 'chat-mode');
  return 'switch';
`, MODE_SWITCH, CHAT_MODE);

const composerState = script(`
  const target = composer();
  if (!target) return null;
  return { text: target.innerText || target.textContent || '', html: target.innerHTML };
`);

const markTools = script(`
  const found = document.querySelector('[data-testid="composer-plus-btn"]')
    || byLabels(clickables(document), arg0);
  return mark(found, 'tools');
`, TOOLS_BUTTON);

const menuItems = script(`
  return Array.from(document.querySelectorAll('[role="menu"], [role="listbox"], [data-radix-popper-content-wrapper]'))
    .filter(isVisible)
    .flatMap((menu) => clickables(menu)).length;
`);

const markDeepResearch = script(`
  const items = Array.from(document.querySelectorAll('[role="menu"], [role="listbox"], [data-radix-popper-content-wrapper]'))
    .filter(isVisible)
    .flatMap((menu) => clickables(menu));
  return mark(byLabels(items, arg0), 'deep-research');
`, DEEP_RESEARCH);

const markSend = script(`
  const found = Array.from(document.querySelectorAll('[data-testid="send-button"], #composer-submit-button'))
    .find((el) => isVisible(el) && isEnabled(el))
    || byLabels(clickables(document), arg0);
  return mark(found, 'send');
`, SEND);

/** The newest assistant reply, once ChatGPT stopped streaming it, or null. */
const settledReply = script(`
  if (document.querySelector('[data-testid="stop-button"]')) return null;
  const turns = Array.from(document.querySelectorAll('[data-message-author-role="assistant"]'));
  const last = turns[turns.length - 1];
  if (!last || last.querySelector('iframe')) return null;
  const text = (last.innerText || '').trim();
  return text ? text.slice(0, 200) : null;
`);

/**
 * Whether the page shows the plan card: the iframe of the card's sandbox, known by its domain or
 * its title, since its content is out of reach.
 */
const planCard = script(`
  return Array.from(document.querySelectorAll('iframe')).some((frame) => {
    if (!isVisible(frame)) return false;
    let host = '';
    try {
      host = new URL(frame.src, location.href).hostname;
    } catch { /* no usable src */ }
    const title = (frame.getAttribute('title') || '').toLowerCase();
    return host === arg0 || host.endsWith('.' + arg0) || arg1.some((label) => title.includes(label));
  });
`, CARD_FRAME_DOMAIN, DEEP_RESEARCH);

/** The lines that read like a usage notice, outside the turns and the navigation. */
const quotaLines = script(`
  const pattern = new RegExp(arg0, 'i');
  const linesOf = (el) => (el.innerText || '').split('\\n').map((line) => line.trim());
  const excluded = new Set(Array.from(document.querySelectorAll(arg1)).flatMap(linesOf));
  return (document.body ? linesOf(document.body) : [])
    .filter((line) => line && pattern.test(line) && !excluded.has(line));
`, QUOTA_PATTERN, NOT_A_NOTICE);

/** The first usage line that is new since `shown` and is not the question itself. */
function newQuotaNotice(lines, shown, query) {
  const line = lines.find((text) => !shown.has(text) && !query.includes(text));
  return line ? line.slice(0, 160) : null;
}

function normalize(text) {
  return String(text || '').replace(/\s+/g, ' ').trim();
}

/** The conversation id in a `/c/<uuid>` or `/g/<project>/c/<uuid>` page URL, or null. */
function conversationOf(href) {
  try {
    const url = new URL(href);
    if (url.hostname !== SITE_DOMAIN) return null;
    const match = url.pathname.match(/^(?:\/g\/[^/]+)?\/c\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\/?$/i);
    return match ? match[1].toLowerCase() : null;
  } catch {
    return null;
  }
}

/**
 * Empties the composer with OpenCLI's key and typing primitives, after closing a menu left open;
 * returns whether it is empty.
 */
async function clearDraft(page) {
  await page.pressKey('Escape');
  await page.typeText(COMPOSER, ' ');
  await page.pressKey('Backspace');
  const state = await page.evaluate(composerState);
  return Boolean(state) && normalize(state.text) === '' && !state.html.includes(DEEP_RESEARCH_APP);
}

/**
 * Operates the page step by step and records each step it completed in `run.steps`. The only
 * step that changes the account, send, runs at most once, and nothing clicks the plan card; a
 * failed step stops the command instead of trying again. Returns normally when it stopped, with
 * `run.problem` saying why.
 */
async function startResearch(page, query, deadline, facts, run) {
  const stop = (problem) => {
    run.problem = problem;
  };
  const passed = () => Date.now() >= deadline;
  const within = (ms) => Math.min(deadline, Date.now() + ms);

  await waitFor(deadline, async () => (await facts.observe(page)) || facts.ready);
  if (facts.facts.signed_out || !facts.ready) {
    if (!passed()) stop('the ChatGPT home page did not show its composer');
    return;
  }

  const mode = await page.evaluate(composerMode);
  if (mode === 'missing') return stop('the composer mode switch offers no chat mode');
  if (mode === 'switch') {
    await page.click(target('chat-mode'));
    const switched = await waitFor(
      within(SWITCH_WAIT_MS),
      async () => (await page.evaluate(composerMode)) === 'chat',
    );
    if (!switched) return stop('the composer did not switch to chat mode');
  }
  run.steps.push('chat_mode');

  // Typing replaces the whole composer, mention included, so the question goes in first.
  run.typed = true;
  await page.typeText(COMPOSER, query);
  const typed = await waitFor(within(3000), async () => {
    const state = await page.evaluate(composerState);
    return state && normalize(state.text) === normalize(query);
  });
  if (!typed) return stop('the composer did not take the question');
  run.steps.push('query');

  if (!await page.evaluate(markTools)) return stop('found no "Add files and more" button');
  await page.click(target('tools'));
  if (!await waitFor(within(MENU_WAIT_MS), () => page.evaluate(menuItems))) {
    return stop('the "Add files and more" menu rendered no items; the Chrome window must be in front');
  }
  if (!await page.evaluate(markDeepResearch)) {
    run.deep_research_missing = true;
    return;
  }
  await page.click(target('deep-research'));
  run.steps.push('deep_research');

  const mentioned = await waitFor(within(MENTION_WAIT_MS), async () => {
    const state = await page.evaluate(composerState);
    return state && state.html.includes(DEEP_RESEARCH_APP)
      && normalize(state.text).includes(normalize(query));
  });
  if (!mentioned) return stop('the composer does not show the question with the Deep Research mention');
  run.steps.push('mention');

  const shown = new Set(await page.evaluate(quotaLines));
  if (!await page.evaluate(markSend)) return stop('found no send button');
  // Recorded before the click: a click that fails halfway may still have sent the question.
  run.steps.push('sent');
  await page.click(target('send'));

  await waitFor(within(CONVERSATION_WAIT_MS), async () => {
    await facts.observe(page);
    run.conversation = conversationOf(facts.facts.url);
    if (run.conversation) return true;
    run.quota_notice = newQuotaNotice(await page.evaluate(quotaLines), shown, query);
    return Boolean(run.quota_notice);
  });
  if (!run.conversation) {
    if (!run.quota_notice && !passed()) stop('the page did not move to the new conversation');
    return;
  }
  run.steps.push('conversation');

  let replySince = null;
  const answered = await waitFor(within(CARD_WAIT_MS), async () => {
    if (await page.evaluate(planCard)) return 'plan';
    run.quota_notice = newQuotaNotice(await page.evaluate(quotaLines), shown, query);
    if (run.quota_notice) return 'quota';
    if (await page.evaluate(settledReply)) {
      replySince = replySince ?? Date.now();
      if (Date.now() - replySince >= REPLY_STABLE_MS) return 'reply';
    } else {
      replySince = null;
    }
    await new Promise((resolve) => setTimeout(resolve, CARD_POLL_MS));
    return null;
  });
  if (!answered || answered === 'quota') {
    if (!answered && !passed()) stop('ChatGPT showed neither a plan nor a reply');
    return;
  }
  run.steps.push('answered');

  if (answered === 'plan') {
    run.steps.push('plan');
    // The countdown was seen starting the research only with the card kept open, so the tab
    // stays on it.
    const until = Math.min(Date.now() + COUNTDOWN_WAIT_MS, deadline - REOPEN_RESERVE_MS);
    if (until > Date.now()) await new Promise((resolve) => setTimeout(resolve, until - Date.now()));
  }

  await reopen(page, deadline, run);
}

/** Opens the conversation again and reports the conversation response the page receives. */
async function reopen(page, deadline, run) {
  const { page: facts, body, completed } = await openConversation(page, run.conversation, deadline);
  run.page = facts;
  if (body !== null) {
    run.response = body;
    run.steps.push('reopened');
  } else {
    run.body_missing = completed;
  }
}

cli({
  site: SITE,
  name: 'start',
  access: 'write',
  description: 'Start one ChatGPT Deep Research and wait out its plan countdown, for forager; never sends twice',
  domain: SITE_DOMAIN,
  strategy: Strategy.COOKIE,
  browser: true,
  navigateBefore: false,
  args: [
    { name: 'query', required: true, valueRequired: true, help: 'The research question' },
    { name: 'timeout', type: 'int', default: 240, help: 'Command timeout in seconds' },
  ],
  func: async (page, kwargs) => {
    const deadline = readDeadline(kwargs.timeout);
    const query = String(kwargs.query);
    const facts = new PageFacts();
    const run = {
      steps: [],
      typed: false,
      conversation: null,
      deep_research_missing: false,
      quota_notice: null,
      problem: null,
      draft_cleared: null,
      page: null,
      response: null,
      body_missing: false,
    };

    await navigate(page, HOME_URL);
    try {
      await startResearch(page, query, deadline, facts, run);
    } catch (error) {
      // Keep the facts gathered so far: after sending, they name the new conversation.
      run.problem = error instanceof Error ? error.message : String(error);
    }
    // The composer keeps its content as a draft across pages, so an unsent question is removed.
    if (run.typed && !run.steps.includes('sent')) {
      try {
        run.draft_cleared = await clearDraft(page);
      } catch {
        run.draft_cleared = false;
      }
    }
    if (!run.page) {
      try {
        await facts.observe(page);
      } catch { /* keep the last facts */ }
    }
    const pageFacts = run.page || facts.facts;
    const finished = run.steps.includes('reopened');
    return envelope({
      page: pageFacts,
      steps: run.steps,
      conversation: run.conversation,
      deep_research_missing: run.deep_research_missing,
      quota_notice: run.quota_notice,
      problem: run.problem ? String(run.problem).slice(0, 300) : null,
      draft_cleared: run.draft_cleared,
      response: run.response,
      body_missing: run.body_missing,
      timed_out: !finished && !run.problem && !run.deep_research_missing && !run.quota_notice
        && !run.body_missing && !pageFacts.signed_out && Date.now() >= deadline,
    });
  },
});
