import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, POLL_MS, PageFacts, SITE, SITE_DOMAIN, STREAM_GENERATE, envelope, readDeadline,
  readRpcCalls, sleep,
} from './shared.js';

// Labels in both interface languages. In the zh-CN interface of 2026-10-09 the tools button
// read 上传和工具, Deep Research sat under 更多工具, a selected tool showed a 取消选择“Deep Research”
// button, and the plan's confirm button kept its English label "Start research". The English
// interface labels are unverified.
const TOOLS_BUTTON = ['上传和工具', '工具', 'tools'];
const MORE_TOOLS = ['更多工具', 'more tools'];
const DEEP_RESEARCH = ['deep research', '深度研究'];
const DESELECT = ['取消选择', 'deselect'];
const SEND = ['发送', 'send message', 'send'];
const CONFIRM = ['start research', '开始研究', '开始调研'];

// The quota wording is an unverified guess. It counts only while Gemini has not answered the
// question, outside the conversation turns and the navigation (whose chat titles and replies
// can mention limits), and only as a line the page did not show before sending. The container
// selectors are unverified as well; forager also ignores a notice once an answer arrived.
const QUOTA_PATTERN = '(reached|hit) (your|the) [^\\n]{0,40}limit|limit for deep research|usage limit|已达到[^\\n]{0,20}(上限|限额)|额度已用(完|尽)|用量已达上限';
const NOT_A_NOTICE = [
  'nav', 'aside', '[role="navigation"]', 'user-query', 'model-response',
  '[class*="conversation-turn"]', '[class*="query-text"]', '[class*="response-text"]',
  'rich-textarea', '[contenteditable="true"]',
].join(', ');

const MENU_WAIT_MS = 10000;
const SELECT_WAIT_MS = 8000;
// The plan card renders shortly after the answer completes; a text answer never shows one.
const CARD_WAIT_MS = 10000;

/** Page-side helpers shared by the scripts: visibility, labels, and clicks. */
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
  const matches = (el, labels) => labels.some((label) => labelOf(el).includes(label));
  // Earlier labels win over later ones, so a short fallback label never beats an exact one.
  const byLabels = (elements, labels, excluded = []) => {
    for (const label of labels) {
      const found = elements.find((el) => labelOf(el).includes(label) && !matches(el, excluded));
      if (found) return found;
    }
    return null;
  };
  const buttons = (root) => Array.from(root.querySelectorAll('button, [role="button"]'))
    .filter((el) => isVisible(el) && isEnabled(el));
  const visibleMenuItems = () => Array.from(document.querySelectorAll(
    '[role="menu"], [role="listbox"], .mat-mdc-menu-panel, .cdk-overlay-pane',
  )).filter(isVisible).flatMap((menu) => Array.from(menu.querySelectorAll(
    'button, [role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"], [role="option"]',
  ))).filter(isVisible);
  const menuItems = () => visibleMenuItems().filter(isEnabled);
`;

function script(body, ...values) {
  return `((${values.map((_, index) => `arg${index}`).join(', ')}) => {${HELPERS}${body}})(${values.map((value) => JSON.stringify(value)).join(', ')})`;
}

const openToolsMenu = script(`
  const target = byLabels(buttons(document), arg0, arg1);
  if (!target) return false;
  if (target.getAttribute('aria-expanded') !== 'true') target.click();
  return true;
`, TOOLS_BUTTON, [...DESELECT, ...DEEP_RESEARCH]);

const menuItemCount = script('return visibleMenuItems().length;');

/** Clicks the first menu item with one of the labels; returns whether it clicked. */
const clickMenuItem = (labels) => script(`
  const item = byLabels(menuItems(), arg0);
  if (!item) return false;
  item.click();
  return true;
`, labels);

/** Whether a visible menu item has one of the labels but is disabled. */
const disabledMenuItem = (labels) => script(`
  return visibleMenuItems().some((el) => !isEnabled(el) && matches(el, arg0));
`, labels);

const deepResearchSelected = script(`
  return buttons(document).some((el) => matches(el, arg0) && matches(el, arg1));
`, DESELECT, DEEP_RESEARCH);

/** Finds the composer, focuses it, and empties it. */
const focusComposer = script(`
  const composer = Array.from(document.querySelectorAll(
    'rich-textarea [contenteditable="true"], div.ql-editor[contenteditable="true"], [contenteditable="true"][role="textbox"]',
  )).find(isVisible);
  if (!composer) return false;
  composer.focus();
  composer.textContent = '';
  composer.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
  return true;
`);

const composerText = script(`
  const composer = Array.from(document.querySelectorAll(
    'rich-textarea [contenteditable="true"], div.ql-editor[contenteditable="true"], [contenteditable="true"][role="textbox"]',
  )).find(isVisible);
  return composer ? (composer.innerText || composer.textContent || '') : null;
`);

const insertTextFallback = (text) => script(`
  document.execCommand('insertText', false, arg0);
  return true;
`, text);

/** Clicks the send button once; returns whether it found one. */
const clickSend = script(`
  const target = buttons(document).find((el) => el.matches('.send-button'))
    || byLabels(buttons(document), arg0, arg1);
  if (!target) return false;
  target.click();
  return true;
`, SEND, [...TOOLS_BUTTON, ...DESELECT]);

/** Clicks the plan's confirm button once; returns whether it found one. */
const clickConfirm = script(`
  const target = buttons(document).find((el) => matches(el, arg0));
  if (!target) return false;
  target.click();
  return true;
`, CONFIRM);

const findConfirm = script(`
  return buttons(document).some((el) => matches(el, arg0));
`, CONFIRM);

/** The lines that read like a quota notice, outside the turns and the navigation. */
const quotaLines = script(`
  const pattern = new RegExp(arg0, 'i');
  const linesOf = (el) => (el.innerText || '').split('\\n').map((line) => line.trim());
  const excluded = new Set(Array.from(document.querySelectorAll(arg1)).flatMap(linesOf));
  return (document.body ? linesOf(document.body) : [])
    .filter((line) => line && pattern.test(line) && !excluded.has(line));
`, QUOTA_PATTERN, NOT_A_NOTICE);

/** The first quota line that is new since `shown` and is not the question itself. */
function newQuotaNotice(lines, shown, query) {
  const line = lines.find((text) => !shown.has(text) && !query.includes(text));
  return line ? line.slice(0, 160) : null;
}

function normalize(text) {
  return String(text || '').replace(/\s+/g, ' ').trim();
}

/** Polls `probe` until it returns a truthy value or `until` passes; returns the last value. */
async function waitFor(until, probe) {
  for (;;) {
    const value = await probe();
    if (value || Date.now() >= until) return value;
    await sleep(POLL_MS);
  }
}

/**
 * Operates the page step by step and records each step it completed in `run.steps`. Every step
 * that changes the account (send, confirm) runs at most once; a failed step stops the command
 * instead of trying again. Returns normally when it stopped, with `run.problem` saying why.
 */
async function startResearch(page, query, deadline, facts, run) {
  const stop = (problem) => {
    run.problem = problem;
  };
  const passed = () => Date.now() >= deadline;
  const within = (ms) => Math.min(deadline, Date.now() + ms);

  await waitFor(deadline, async () => (await facts.observe(page)) || facts.ready);
  if (facts.facts.signed_out || facts.facts.notice || !facts.ready) {
    if (!passed()) stop('the Gemini app did not show its composer');
    return;
  }

  if (!await page.evaluate(openToolsMenu)) return stop('found no tools button');
  if (!await waitFor(within(MENU_WAIT_MS), () => page.evaluate(menuItemCount))) {
    return stop('the tools menu rendered no items; the Chrome window must be in front');
  }
  run.steps.push('tools_menu');

  let selected = await page.evaluate(clickMenuItem(DEEP_RESEARCH));
  if (!selected && await page.evaluate(clickMenuItem(MORE_TOOLS))) {
    selected = await waitFor(within(SELECT_WAIT_MS), () => page.evaluate(clickMenuItem(DEEP_RESEARCH)));
  }
  if (!selected) {
    if (await page.evaluate(disabledMenuItem(DEEP_RESEARCH))) run.deep_research_disabled = true;
    else run.deep_research_missing = !passed();
    return;
  }
  if (!await waitFor(within(SELECT_WAIT_MS), () => page.evaluate(deepResearchSelected))) {
    return stop('Deep Research did not show as selected');
  }
  run.steps.push('deep_research');

  if (!await page.evaluate(focusComposer)) return stop('found no composer');
  if (page.insertText) await page.insertText(query);
  else await page.evaluate(insertTextFallback(query));
  const typed = await waitFor(within(3000), async () => normalize(await page.evaluate(composerText)) === normalize(query));
  if (!typed) return stop('the composer did not take the question');
  run.steps.push('query');

  const shown = new Set(await page.evaluate(quotaLines));
  if (!await page.evaluate(clickSend)) return stop('found no send button');
  run.steps.push('sent');

  await waitFor(deadline, async () => {
    await facts.observe(page);
    if (facts.completions >= 1) return true;
    run.quota_notice = newQuotaNotice(await page.evaluate(quotaLines), shown, query);
    return Boolean(run.quota_notice);
  });
  if (facts.completions < 1) return;
  // OpenCLI asks Chrome for a body on the request's loadingFinished event, which Chrome sends
  // before the page sees the completion, and a tab answers DevTools commands in order. One more
  // page round trip therefore returns only after the body request has been answered.
  await facts.observe(page);
  run.steps.push('answered');

  if (!await waitFor(within(CARD_WAIT_MS), () => page.evaluate(findConfirm))) {
    if (!passed()) stop('Gemini showed no "Start research" button');
    return;
  }
  if (!await page.evaluate(clickConfirm)) return stop('the "Start research" button went away');
  run.steps.push('confirmed');

  const started = await waitFor(deadline, async () => {
    await facts.observe(page);
    return facts.completions >= 2;
  });
  if (!started) return;
  await facts.observe(page);
  run.steps.push('started');
}

cli({
  site: SITE,
  name: 'start',
  access: 'write',
  description: 'Start one Gemini Deep Research and confirm its plan, for forager; never sends twice',
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
    const facts = new PageFacts(STREAM_GENERATE);
    const run = {
      steps: [],
      deep_research_missing: false,
      deep_research_disabled: false,
      quota_notice: null,
      problem: null,
    };

    // Capture first: the answer streams in while the page is still busy.
    await page.startNetworkCapture(SITE_DOMAIN);
    await page.goto(APP_URL, { waitUntil: 'load', settleMs: 1000 });
    try {
      await startResearch(page, query, deadline, facts, run);
    } catch (error) {
      // Keep the facts gathered so far: after sending, they name the new conversation.
      run.problem = error instanceof Error ? error.message : String(error);
    }
    try {
      await facts.observe(page);
    } catch { /* keep the last facts */ }
    // Reading drains the capture; every awaited answer has completed by now.
    const calls = await readRpcCalls(page, STREAM_GENERATE);
    const finished = run.steps.includes('started');
    return envelope({
      page: facts.facts,
      steps: run.steps,
      deep_research_missing: run.deep_research_missing,
      deep_research_disabled: run.deep_research_disabled,
      quota_notice: run.quota_notice,
      problem: run.problem ? String(run.problem).slice(0, 300) : null,
      plan_response: calls[0] ? calls[0].body : null,
      confirm_response: calls[1] ? calls[1].body : null,
      timed_out: !finished && !run.problem && !run.deep_research_missing
        && !run.deep_research_disabled && !run.quota_notice
        && !facts.facts.signed_out && Date.now() >= deadline,
    });
  },
});
