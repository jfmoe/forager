import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  HOME_URL, PageFacts, SITE, SITE_DOMAIN, clickTarget, confirmWithin, envelope, inputFailureOf,
  navigate, openConversation, pressNativeKey, readDeadline, typeText, waitFor,
} from './shared.js';

// Labels in both interface languages. The zh-CN interface of 2026-10-10 showed a 撰写器模式
// switch with 聊天 and 工作, an 添加文件等内容 button whose chat-mode menu holds 深度研究, and a
// plan card with 编辑, 取消, and a counting-down 开始. The card lives in a cross-origin iframe
// from web-sandbox.oaiusercontent.com titled 深度研究, which OpenCLI can neither inspect nor
// click into, so the adapter never clicks 开始 and waits out the countdown instead. The zh-CN
// path through send and the countdown passed a live run on 2026-10-11; the English labels and
// the reply and usage-notice detection are unverified.
const COMPOSER = '[contenteditable="true"][role="textbox"]';
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

// The add-files menu renders its items in plain divs grouped by `data-mention-section-id`,
// with no menu role (live 2026-10-11).
const MENU = '[role="menu"], [role="listbox"], [data-radix-popper-content-wrapper], [data-mention-section-id]';
const MENU_WAIT_MS = 10000;
const SWITCH_WAIT_MS = 5000;
const MENTION_WAIT_MS = 5000;
// How long a click into the composer, a key that edits it, or the send click has to show its
// effect.
const COMPOSER_CHECK_MS = 5000;
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
  // A button or menu item can nest another that carries the same label; the pointer hits both,
  // so only the outermost one counts.
  const outermost = (elements) => elements
    .filter((el) => !elements.some((other) => other !== el && other.contains(el)));
  // The elements with the earliest label that any of them carries, so a short fallback label
  // never competes with an exact one.
  const labelled = (elements, labels) => {
    for (const label of labels) {
      const found = elements.filter((el) => labelOf(el).includes(label));
      if (found.length > 0) return outermost(found);
    }
    return [];
  };
  const clickables = (root) => Array.from(root.querySelectorAll(
    'button, [role="button"], [role="radio"], [role="tab"], [role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"], [role="option"]',
  )).filter((el) => isVisible(el) && isEnabled(el));
  const isSelected = (el) => ['aria-checked', 'aria-selected', 'aria-pressed']
    .some((name) => el.getAttribute(name) === 'true')
    || ['on', 'active', 'checked'].includes(el.getAttribute('data-state') || '');
  const composers = () => Array.from(document.querySelectorAll('${COMPOSER}')).filter(isVisible);
  const composer = () => composers()[0];
  // The menu containers nest (a popper wrapper holds the menu), so an item can be found through
  // more than one of them.
  const menuItems = () => [...new Set(Array.from(document.querySelectorAll('${MENU}'))
    .filter(isVisible)
    .flatMap((menu) => clickables(menu)))];
  // The mode switch labelled with one of \`switchLabels\`, and its options labelled with one of
  // \`chatLabels\` (empty when it has none); undefined when the page shows no mode switch.
  const chatOptions = (switchLabels, chatLabels) => {
    const group = Array.from(document.querySelectorAll('[aria-label]'))
      .filter(isVisible)
      .find((el) => switchLabels.some((label) => (el.getAttribute('aria-label') || '').toLowerCase().includes(label)));
    return group ? labelled(clickables(group), chatLabels) : undefined;
  };
`;

function script(body, ...values) {
  return `((${values.map((_, index) => `arg${index}`).join(', ')}) => {${HELPERS}${body}})(${values.map((value) => JSON.stringify(value)).join(', ')})`;
}

/**
 * The composer mode: `chat`; `switch` when the chat option is there to click; `missing` when the
 * mode switch has no chat option; `none` when the page shows no mode switch, as for an account
 * without the work mode.
 */
const composerMode = script(`
  const chat = chatOptions(arg0, arg1);
  if (chat === undefined) return 'none';
  if (chat.length === 0) return 'missing';
  return chat.some(isSelected) ? 'chat' : 'switch';
`, MODE_SWITCH, CHAT_MODE);

const composerState = script(`
  const target = composer();
  if (!target) return null;
  return { text: target.innerText || target.textContent || '', html: target.innerHTML };
`);

const composerFocused = script(`
  const target = composer();
  return Boolean(target) && target.contains(document.activeElement);
`);

const menuItemCount = script(`
  return menuItems().length;
`);

// Page expressions for the click targets, each yielding the candidate elements; the click picks
// the one a pointer hits, or fails when none or several are.
const CHAT_MODE_TARGET = script('return chatOptions(arg0, arg1) || [];', MODE_SWITCH, CHAT_MODE);

const COMPOSER_TARGET = script('return composers();');

const TOOLS_TARGET = script(`
  const plus = Array.from(document.querySelectorAll('[data-testid="composer-plus-btn"]'));
  return plus.length > 0 ? plus : labelled(clickables(document), arg0);
`, TOOLS_BUTTON);

const DEEP_RESEARCH_TARGET = script('return labelled(menuItems(), arg0);', DEEP_RESEARCH);

const SEND_TARGET = script(`
  const send = Array.from(document.querySelectorAll('[data-testid="send-button"], #composer-submit-button'))
    .filter((el) => isVisible(el) && isEnabled(el));
  return send.length > 0 ? outermost(send) : labelled(clickables(document), arg0);
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

/** Whether the composer shows neither text nor the Deep Research mention. */
async function composerEmpty(page) {
  const state = await page.evaluate(composerState);
  return Boolean(state) && normalize(state.text) === '' && !state.html.includes(DEEP_RESEARCH_APP);
}

/** Clicks into the composer and confirms it took the focus; returns null or why it could not. */
async function focusComposer(page, deadline) {
  const failure = await inputFailureOf(() => clickTarget(page, COMPOSER_TARGET, { deadline }));
  if (failure) return `could not click the composer (${failure})`;
  const focused = await confirmWithin(() => page.evaluate(composerFocused), COMPOSER_CHECK_MS, deadline);
  return focused ? null : 'the composer did not take the focus (click_missed)';
}

/** Selects everything in the focused composer and deletes it; returns whether it is empty. */
async function emptyComposer(page, deadline) {
  await pressNativeKey(page, 'selectAll', { deadline });
  await pressNativeKey(page, 'backspace', { deadline });
  return confirmWithin(() => composerEmpty(page), COMPOSER_CHECK_MS, deadline);
}

/** Empties the composer, after closing a menu left open; returns whether it is empty. */
async function clearDraft(page, deadline) {
  await pressNativeKey(page, 'escape', { deadline });
  if (await focusComposer(page, deadline)) return false;
  return emptyComposer(page, deadline);
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
  const click = (targets) => inputFailureOf(() => clickTarget(page, targets, { deadline }));

  await waitFor(deadline, async () => (await facts.observe(page)) || facts.ready);
  if (facts.facts.signed_out || !facts.ready) {
    if (!passed()) stop('the ChatGPT home page did not show its composer');
    return;
  }

  const mode = await page.evaluate(composerMode);
  if (mode === 'missing') return stop('the composer mode switch offers no chat mode');
  if (mode === 'switch') {
    const failure = await click(CHAT_MODE_TARGET);
    if (failure) return stop(`could not click the chat mode (${failure})`);
    const switched = await waitFor(
      within(SWITCH_WAIT_MS),
      async () => (await page.evaluate(composerMode)) === 'chat',
    );
    if (!switched) return stop('the composer did not switch to chat mode (click_missed)');
  }
  run.steps.push('chat_mode');

  // The question goes in before the mention. Typing only inserts at the caret, so an earlier
  // draft left in the composer is deleted first.
  const unfocused = await focusComposer(page, deadline);
  if (unfocused) return stop(unfocused);
  run.typed = true;
  if (!await composerEmpty(page) && !await emptyComposer(page, deadline)) {
    return stop('could not empty the earlier draft in the composer');
  }
  await typeText(page, query, { deadline });
  const typed = await waitFor(within(3000), async () => {
    const state = await page.evaluate(composerState);
    return state && normalize(state.text) === normalize(query);
  });
  if (!typed) return stop('the composer did not take the question');
  run.steps.push('query');

  const toolsFailure = await click(TOOLS_TARGET);
  if (toolsFailure) return stop(`could not click the "Add files and more" button (${toolsFailure})`);
  if (!await waitFor(within(MENU_WAIT_MS), () => page.evaluate(menuItemCount))) {
    return stop('the "Add files and more" menu rendered no items (click_missed); the Chrome window must be in front');
  }
  if (!await page.evaluate(`${DEEP_RESEARCH_TARGET}.length > 0`)) {
    run.deep_research_missing = true;
    return;
  }
  const deepResearchFailure = await click(DEEP_RESEARCH_TARGET);
  if (deepResearchFailure) return stop(`could not click the Deep Research entry (${deepResearchFailure})`);
  run.steps.push('deep_research');

  const mentioned = await waitFor(within(MENTION_WAIT_MS), async () => {
    const state = await page.evaluate(composerState);
    return state && state.html.includes(DEEP_RESEARCH_APP)
      && normalize(state.text).includes(normalize(query));
  });
  if (!mentioned) return stop('the composer does not show the question with the Deep Research mention (click_missed)');
  run.steps.push('mention');

  const shown = new Set(await page.evaluate(quotaLines));
  // Recorded before the click: a click that fails halfway may still have sent the question. An
  // input failure comes before the pointer presses, so nothing was sent then.
  run.steps.push('sent');
  const sendFailure = await click(SEND_TARGET);
  if (sendFailure) {
    run.steps.pop();
    return stop(`could not click the send button (${sendFailure})`);
  }
  // ChatGPT empties the composer, or moves to the new conversation, once it takes the question;
  // a usage notice instead is the answer to the send as well.
  const taken = await confirmWithin(async () => {
    await facts.observe(page);
    if (conversationOf(facts.facts.url)) return true;
    if (newQuotaNotice(await page.evaluate(quotaLines), shown, query)) return true;
    const state = await page.evaluate(composerState);
    return !state || !normalize(state.text).includes(normalize(query));
  }, COMPOSER_CHECK_MS, deadline);
  if (!taken) {
    if (!passed()) stop('the composer still held the question after the send click (click_missed)');
    return;
  }

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
        run.draft_cleared = await clearDraft(page, deadline);
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
