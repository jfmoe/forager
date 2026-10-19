import { cli, Strategy } from '@jackwener/opencli/registry';

import {
  APP_URL, PageFacts, SITE, SITE_DOMAIN, STREAM_GENERATE, clickTarget, confirmWithin, envelope,
  inputFailureOf, navigate, pressNativeKey, readDeadline, readTrackedCalls, typeText, waitFor,
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
// How long the composer has to take focus, empty, or show the typed question.
const COMPOSER_WAIT_MS = 3000;
// How long a send or confirm click has to show its effect on the page.
const CLICK_CHECK_MS = 5000;
// The plan card renders after the answer completes, live sometimes later than 10 s; a text
// answer never shows one.
const CARD_WAIT_MS = 45000;

/** Page-side helpers shared by the scripts: visibility, labels, and the elements to act on. */
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
  // A menu item or button can nest another that carries the same label; the pointer hits both,
  // so only the outermost one counts.
  const outermost = (elements) => elements
    .filter((el) => !elements.some((other) => other !== el && other.contains(el)));
  // The elements with the earliest label that any of them carries, so a short fallback label
  // never competes with an exact one.
  const labelled = (elements, labels, excluded = []) => {
    for (const label of labels) {
      const found = elements.filter((el) => labelOf(el).includes(label) && !matches(el, excluded));
      if (found.length > 0) return outermost(found);
    }
    return [];
  };
  const buttons = (root) => Array.from(root.querySelectorAll('button, [role="button"]'))
    .filter((el) => isVisible(el) && isEnabled(el));
  const visibleMenuItems = () => Array.from(document.querySelectorAll(
    '[role="menu"], [role="listbox"], .mat-mdc-menu-panel, .cdk-overlay-pane',
  )).filter(isVisible).flatMap((menu) => Array.from(menu.querySelectorAll(
    'button, [role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"], [role="option"]',
  ))).filter(isVisible);
  const menuItems = () => visibleMenuItems().filter(isEnabled);
  const composer = () => Array.from(document.querySelectorAll(
    'rich-textarea [contenteditable="true"], div.ql-editor[contenteditable="true"], [contenteditable="true"][role="textbox"]',
  )).find(isVisible);
`;

/** A page expression that runs `body` with the helpers in scope and `values` as `arg0`, …. */
function script(body, ...values) {
  return `((${values.map((_, index) => `arg${index}`).join(', ')}) => {${HELPERS}${body}})(${values.map((value) => JSON.stringify(value)).join(', ')})`;
}

/** The tools button: `missing`, `open`, or `closed`. */
const toolsButton = script(`
  const [target] = labelled(buttons(document), arg0, arg1);
  if (!target) return 'missing';
  return target.getAttribute('aria-expanded') === 'true' ? 'open' : 'closed';
`, TOOLS_BUTTON, [...DESELECT, ...DEEP_RESEARCH]);
const TOOLS_TARGET = script('return labelled(buttons(document), arg0, arg1);', TOOLS_BUTTON, [...DESELECT, ...DEEP_RESEARCH]);

const menuItemCount = script('return visibleMenuItems().length;');
const menuLabels = script('return visibleMenuItems().map(labelOf).join("\\n");');

/** The enabled menu items with the earliest of the labels, for a click. */
const menuItemTarget = (labels) => script('return labelled(menuItems(), arg0);', labels);

/** Whether a menu item with one of the labels shows, enabled or, with `disabled`, not. */
const menuItemShown = (labels, disabled) => script(`
  return visibleMenuItems().some((el) => matches(el, arg0) && (arg1 ? !isEnabled(el) : isEnabled(el)));
`, labels, disabled);

const deepResearchSelected = script(`
  return buttons(document).some((el) => matches(el, arg0) && matches(el, arg1));
`, DESELECT, DEEP_RESEARCH);

const COMPOSER_TARGET = script('const target = composer(); return target ? [target] : [];');

const composerFocused = script(`
  const target = composer();
  return Boolean(target) && target.contains(document.activeElement);
`);

const composerText = script(`
  const target = composer();
  return target ? (target.innerText || target.textContent || '') : null;
`);

const SEND_TARGET = script(`
  const send = buttons(document).filter((el) => el.matches('.send-button'));
  return send.length > 0 ? outermost(send) : labelled(buttons(document), arg0, arg1);
`, SEND, [...TOOLS_BUTTON, ...DESELECT]);

const CONFIRM_TARGET = script('return outermost(buttons(document).filter((el) => matches(el, arg0)));', CONFIRM);
const findConfirm = script('return buttons(document).some((el) => matches(el, arg0));', CONFIRM);

/** Why no confirm button counts as clickable: the label matches and what rules each out. */
const describeConfirm = script(`
  const found = Array.from(document.querySelectorAll('button, [role="button"]'))
    .filter((el) => matches(el, arg0));
  if (found.length === 0) return 'no button is labelled like it';
  return found.slice(0, 3).map((el) => {
    const reasons = [];
    if (el.closest('[aria-hidden="true"]')) reasons.push('inside aria-hidden');
    if (el.closest('[hidden]')) reasons.push('inside hidden');
    if (!isVisible(el)) reasons.push('not visible');
    if (!isEnabled(el)) reasons.push('disabled');
    return (reasons.join(', ') || 'clickable now');
  }).join('; ') + ' (' + found.length + ' labelled like it)';
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

/**
 * Clicks the one target of a step that changes the account, and records `step` as soon as the
 * click may have reached the page, so a later failure never claims that nothing happened.
 * Returns null or why it could not click.
 */
async function clickOnce(page, targets, deadline, run, step, names = {}) {
  let failure;
  try {
    failure = await inputFailureOf(() => clickTarget(page, targets, { deadline }), names);
  } catch (error) {
    run.steps.push(step);
    throw error;
  }
  if (!failure) run.steps.push(step);
  return failure;
}

/**
 * Operates the page step by step and records each step it completed in `run.steps`. Every step
 * that changes the account (send, confirm) runs at most once; a failed step stops the command
 * instead of trying again. Returns normally when it stopped, with `run.problem` saying why.
 */
async function startResearch(page, query, deadline, facts, run) {
  const passed = () => Date.now() >= deadline;
  // A step cut short by the deadline reports the deadline, not its own reason.
  const stop = (problem) => {
    if (!passed()) run.problem = problem;
  };
  const within = (ms) => Math.min(deadline, Date.now() + ms);
  const click = (targets) => inputFailureOf(() => clickTarget(page, targets, { deadline }));

  await waitFor(deadline, async () => (await facts.observe(page)) || facts.ready);
  if (facts.facts.signed_out || facts.facts.notice || !facts.ready) {
    if (!passed()) stop('the Gemini app did not show its composer');
    return;
  }

  const tools = await page.evaluate(toolsButton);
  if (tools === 'missing') return stop('found no tools button');
  if (tools === 'closed') {
    const failure = await click(TOOLS_TARGET);
    if (failure) return stop(`could not click the tools button: ${failure}`);
  }
  if (!await waitFor(within(MENU_WAIT_MS), () => page.evaluate(menuItemCount))) {
    return stop('click_missed: the tools menu rendered no items; the Chrome window must be in front');
  }
  run.steps.push('tools_menu');

  if (!await page.evaluate(menuItemShown(DEEP_RESEARCH, false))
    && await page.evaluate(menuItemShown(MORE_TOOLS, false))) {
    const before = await page.evaluate(menuLabels);
    const failure = await click(menuItemTarget(MORE_TOOLS));
    if (failure) return stop(`could not click "More tools": ${failure}`);
    const revealed = async () => await page.evaluate(menuItemShown(DEEP_RESEARCH, false))
      || page.evaluate(menuItemShown(DEEP_RESEARCH, true));
    if (!await confirmWithin(revealed, SELECT_WAIT_MS, deadline)
      && await page.evaluate(menuLabels) === before) {
      return stop('click_missed: "More tools" showed no further items');
    }
  }
  if (!await page.evaluate(menuItemShown(DEEP_RESEARCH, false))) {
    if (await page.evaluate(menuItemShown(DEEP_RESEARCH, true))) run.deep_research_disabled = true;
    else run.deep_research_missing = !passed();
    return;
  }
  const chosen = await click(menuItemTarget(DEEP_RESEARCH));
  if (chosen) return stop(`could not click Deep Research: ${chosen}`);
  if (!await confirmWithin(() => page.evaluate(deepResearchSelected), SELECT_WAIT_MS, deadline)) {
    return stop('click_missed: Deep Research did not show as selected');
  }
  run.steps.push('deep_research');

  const focused = await inputFailureOf(() => clickTarget(page, COMPOSER_TARGET, { deadline }), {
    no_target: 'found no composer',
  });
  if (focused) return stop(`could not click the composer: ${focused}`);
  if (!await confirmWithin(() => page.evaluate(composerFocused), COMPOSER_WAIT_MS, deadline)) {
    return stop('click_missed: the composer did not take focus');
  }
  // A draft left in the composer would go out with the question.
  if (normalize(await page.evaluate(composerText)) !== '') {
    const cleared = await inputFailureOf(async () => {
      await pressNativeKey(page, 'selectAll', { deadline });
      await pressNativeKey(page, 'backspace', { deadline });
    });
    if (cleared) return stop(`could not clear the composer: ${cleared}`);
    const empty = async () => normalize(await page.evaluate(composerText)) === '';
    if (!await confirmWithin(empty, COMPOSER_WAIT_MS, deadline)) return stop('the composer did not clear');
  }
  const typing = await inputFailureOf(() => typeText(page, query, { deadline }));
  if (typing) return stop(`could not type the question: ${typing}`);
  const typed = async () => normalize(await page.evaluate(composerText)) === normalize(query);
  if (!await confirmWithin(typed, COMPOSER_WAIT_MS, deadline)) return stop('the composer did not take the question');
  run.steps.push('query');

  const shown = new Set(await page.evaluate(quotaLines));
  const sending = await clickOnce(page, SEND_TARGET, deadline, run, 'sent', { no_target: 'found no send button' });
  if (sending) return stop(`could not click the send button: ${sending}`);
  // Gemini empties the composer once it takes the question.
  const left = await confirmWithin(async () => {
    await facts.observe(page);
    return facts.completions >= 1 || normalize(await page.evaluate(composerText)) !== normalize(query);
  }, CLICK_CHECK_MS, deadline);
  if (!left) return stop('click_missed: the composer still held the question after the send click');

  await waitFor(deadline, async () => {
    await facts.observe(page);
    if (facts.completions >= 1) return true;
    run.quota_notice = newQuotaNotice(await page.evaluate(quotaLines), shown, query);
    return Boolean(run.quota_notice);
  });
  if (facts.completions < 1) return;
  await facts.settle(page);
  run.steps.push('answered');

  if (!await waitFor(within(CARD_WAIT_MS), () => page.evaluate(findConfirm))) {
    if (!passed()) stop(`Gemini showed no "Start research" button: ${await page.evaluate(describeConfirm)}`);
    return;
  }
  const confirming = await clickOnce(page, CONFIRM_TARGET, deadline, run, 'confirmed', {
    no_target: 'the "Start research" button went away',
  });
  if (confirming) return stop(confirming);
  const accepted = await confirmWithin(async () => {
    await facts.observe(page);
    return facts.completions >= 2 || !await page.evaluate(findConfirm);
  }, CLICK_CHECK_MS, deadline);
  if (!accepted) return stop('click_missed: the "Start research" button stayed after the click');

  const started = await waitFor(deadline, async () => {
    await facts.observe(page);
    return facts.completions >= 2;
  });
  if (!started) return;
  await facts.settle(page);
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

    // The capture starts before the navigation: the answer streams in while the page is busy.
    await navigate(page, APP_URL);
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
    const calls = await readTrackedCalls(page, STREAM_GENERATE);
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
