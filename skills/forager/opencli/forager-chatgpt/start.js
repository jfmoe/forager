import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  HOME_URL, PageFacts, SITE, SITE_DOMAIN, conversationPath, envelope, readConversationReads,
  readDeadline, waitFor,
} from './shared.js';

// Labels in both interface languages. The zh-CN interface of 2026-10-10 showed a 撰写器模式
// switch with 聊天 and 工作, an 添加文件等内容 button whose chat-mode menu holds 深度研究, and a
// plan card with 编辑, 取消, and a counting-down 开始. The English labels, the send button, the
// reply and usage-notice detection, and where the Start button lives (the main document or the
// card's cross-origin sandbox iframe) are unverified; the live acceptance run confirms them.
const COMPOSER = '#prompt-textarea';
const MODE_SWITCH = ['撰写器模式', 'composer mode'];
const CHAT_MODE = ['聊天', 'chat'];
const TOOLS_BUTTON = ['添加文件等内容', 'add files and more'];
const DEEP_RESEARCH = ['深度研究', 'deep research'];
const SEND = ['发送提示', '发送', 'send prompt', 'send'];
const DEEP_RESEARCH_APP = 'connector_openai_deep_research';
// "开始" or "Start", optionally followed by the countdown.
const START_PATTERN = '^(开始|start)\\s*[(（]?\\s*\\d*\\s*(s|秒)?\\s*[)）]?$';

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
// The plan card showed within 15 s live. A reply only counts once it stayed without a Start
// button this long, so a card still loading never reads as a reply.
const CARD_WAIT_MS = 60000;
const REPLY_STABLE_MS = 10000;
const START_GONE_WAIT_MS = 15000;
const CARD_POLL_MS = 2000;

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
 * Finds the plan's Start button in OpenCLI's accessibility snapshot, which also covers the
 * card's cross-origin iframe. Returns `{ref, document}` with the snapshot ref OpenCLI clicks,
 * or null.
 */
async function findStart(page) {
  const snapshot = await page.snapshot({ source: 'ax', interactive: true });
  const pattern = new RegExp(START_PATTERN, 'i');
  let document = 'main';
  for (const line of String(snapshot).split('\n')) {
    const frame = line.match(/^frame (".*"):$/);
    if (frame) {
      try {
        document = new URL(JSON.parse(frame[1])).hostname;
      } catch {
        document = 'frame';
      }
      continue;
    }
    const button = line.match(/^\s*\[(\d+)\]button ("(?:[^"\\]|\\.)*")/);
    if (!button || /\bdisabled\b/.test(line.slice(button[0].length))) continue;
    if (pattern.test(normalize(JSON.parse(button[2])))) return { ref: button[1], document };
  }
  return null;
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
 * Operates the page step by step and records each step it completed in `run.steps`. Every step
 * that changes the account (send, Start) runs at most once; a failed step stops the command
 * instead of trying again. Returns normally when it stopped, with `run.problem` saying why.
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
  let start = null;
  const answered = await waitFor(within(CARD_WAIT_MS), async () => {
    start = await findStart(page);
    if (start) return 'plan';
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
    run.start_button = start.document;
    try {
      await page.click(start.ref);
    } catch (error) {
      return stop(`the Start button could not be clicked: ${error instanceof Error ? error.message : String(error)}`);
    }
    run.steps.push('started');
    // Leave the page only after the card took the click.
    await waitFor(within(START_GONE_WAIT_MS), async () => !(await findStart(page)));
  }

  await reopen(page, deadline, run);
}

/**
 * Opens the conversation again and reports the conversation response the page receives once it
 * has fully arrived: capture first, because the page reads the conversation while the
 * navigation is still settling.
 */
async function reopen(page, deadline, run) {
  const path = conversationPath(run.conversation);
  const facts = new PageFacts(path);
  await page.startNetworkCapture(SITE_DOMAIN);
  await page.goto(`${HOME_URL}c/${encodeURIComponent(run.conversation)}`, {
    waitUntil: 'load',
    settleMs: 1000,
  });
  const ended = await waitFor(deadline, async () => {
    if (await facts.observe(page)) return 'page';
    return facts.completions > 0 ? 'completed' : null;
  });
  const completed = ended === 'completed';
  if (completed) await facts.settle(page);
  const reads = await readConversationReads(page, path);
  const answered = reads.find((read) => read.body !== null);
  run.page = facts.facts;
  if (answered) {
    run.response = answered.body;
    run.steps.push('reopened');
  } else {
    run.body_missing = completed;
  }
}

cli({
  site: SITE,
  name: 'start',
  access: 'write',
  description: 'Start one ChatGPT Deep Research and click its plan\'s Start button, for forager; never sends twice',
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
      start_button: null,
      page: null,
      response: null,
      body_missing: false,
    };

    await page.goto(HOME_URL, { waitUntil: 'load', settleMs: 1000 });
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
      // Not judged by forager; it records for the live acceptance run where the Start button was.
      start_button: run.start_button,
      response: run.response,
      body_missing: run.body_missing,
      timed_out: !finished && !run.problem && !run.deep_research_missing && !run.quota_notice
        && !run.body_missing && !pageFacts.signed_out && Date.now() >= deadline,
    });
  },
});
