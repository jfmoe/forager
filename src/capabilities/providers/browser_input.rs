//! Native input: every action that changes the page (pointer moves and hovers, clicks, wheel
//! scrolls, keys, and text) goes through the browser's input pipeline as CDP `Input.*` events, so
//! the page sees trusted events with coordinates and a full pointer sequence. Nothing here calls
//! `element.click()`, `dispatchEvent`, `execCommand`, or assigns or scrolls a scroll position
//! from script; reading the page to find a target is not an action.
//!
//! The pacing is a person's: a random pause of 0.3–1.5 seconds before each action, a hover
//! before each click, a pointer that moves to its target in several steps, a wheel that turns in
//! small notches with pauses between bursts, and short text typed one character at a time, CJK
//! text in the small groups an input method commits. No input goes out after the read deadline. An action that cannot be
//! done natively fails with its reason and never falls back to a scripted one.
//!
//! A click aims only at a target that stands still, and re-aims when the target moves or is
//! covered during the hover, but presses at most once: nothing is retried after the press.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::Page;
use crate::net::AttemptFailure;

const INPUT_PAUSE_MS: (f64, f64) = (300.0, 1500.0);
const HOVER_DWELL_MS: (f64, f64) = (80.0, 250.0);
const PRESS_MS: (f64, f64) = (50.0, 130.0);
const KEY_GAP_MS: (f64, f64) = (30.0, 90.0);
// An input method commits a few characters at a time, after the user picks them.
const COMMIT_GAP_MS: (f64, f64) = (100.0, 300.0);
const COMMIT_CHARACTERS: (usize, usize) = (1, 3);
// A mouse wheel turns in notches of about 100 px, a few at a time with a pause between bursts.
const WHEEL_NOTCH_PX: (f64, f64) = (80.0, 120.0);
const WHEEL_GAP_MS: (f64, f64) = (30.0, 90.0);
const WHEEL_BURST: (usize, usize) = (3, 8);
const WHEEL_BURST_PAUSE_MS: (f64, f64) = (200.0, 700.0);
// At the slowest pacing 12 notches take 2.8 seconds, which keeps five pages of search results
// or comments within the default attempt timeout when each page takes one wheel turn.
const MAX_WHEEL_NOTCHES: usize = 12;
const POINTER_STEPS: (usize, usize) = (8, 18);
const POINTER_STEP_MS: (f64, f64) = (8.0, 22.0);
const POINTER_CURVE_PX: f64 = 60.0;
const MAX_TYPED_CHARACTERS: usize = 200;
// A target may render a moment after the action that reveals it.
const LOCATE_WITHIN: Duration = Duration::from_secs(2);
const LOCATE_POLL: Duration = Duration::from_millis(200);
const MAX_SCROLLS_INTO_VIEW: usize = 8;
// A target counts as standing still when two readings this far apart agree.
const STEADY_GAP: Duration = Duration::from_millis(100);
const STEADY_PX: f64 = 0.5;
// How many times a click aims again after its recheck finds the target moved or covered.
const MAX_REAIMS: usize = 2;
// How much of the element under the pointer a failed recheck reports.
const MAX_HIT_CHARACTERS: usize = 80;
// Chromium animates a wheel scroll past its last event, so positions read too early are stale.
const SCROLL_SETTLE_POLL: Duration = Duration::from_millis(50);
const SCROLL_SETTLE_WITHIN: Duration = Duration::from_millis(1500);
// How far from the center a click may land, as a fraction of the target's size.
const CLICK_JITTER: f64 = 0.25;

/// Why a native action did not happen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputFailure {
    /// No element the targets name is visible.
    NoTarget,
    /// More than one element the targets name can be hit.
    AmbiguousTarget,
    /// The target is in view but something covers it.
    ObscuredTarget,
    /// The wheel cannot bring the target into view.
    OutOfView,
    /// After the hover, the pointer no longer hits the target.
    TargetMoved,
    /// The click did not have its effect on the page.
    ClickMissed,
    /// The read deadline passed before the action.
    DeadlinePassed,
}

impl InputFailure {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::NoTarget => "no_target",
            Self::AmbiguousTarget => "ambiguous_target",
            Self::ObscuredTarget => "obscured_target",
            Self::OutOfView => "out_of_view",
            Self::TargetMoved => "target_moved",
            Self::ClickMissed => "click_missed",
            Self::DeadlinePassed => "deadline_passed",
        }
    }

    fn from_reason(reason: &str) -> Self {
        match reason {
            "ambiguous_target" => Self::AmbiguousTarget,
            "obscured_target" => Self::ObscuredTarget,
            "out_of_view" => Self::OutOfView,
            "target_moved" => Self::TargetMoved,
            _ => Self::NoTarget,
        }
    }
}

/// A native action that did not happen on the page: why, and for a click stopped before its
/// press, what the pointer hit and how far the target had moved since it was located. The
/// diagnosis names elements by tag and class only, never by their text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Missed {
    pub(crate) failure: InputFailure,
    pub(crate) diagnosis: Option<String>,
}

impl Missed {
    /// `name`, the caller's name for the failure, followed by the diagnosis.
    pub(crate) fn describe(&self, name: &str) -> String {
        match &self.diagnosis {
            Some(diagnosis) => format!("{name} [{diagnosis}]"),
            None => name.to_owned(),
        }
    }
}

/// A native action that failed: on the page, or in the browser connection.
#[derive(Debug)]
pub(crate) enum InputError {
    Failed(Missed),
    Transport(AttemptFailure),
}

impl From<AttemptFailure> for InputError {
    fn from(failure: AttemptFailure) -> Self {
        Self::Transport(failure)
    }
}

impl From<InputFailure> for InputError {
    fn from(failure: InputFailure) -> Self {
        Self::Failed(Missed {
            failure,
            diagnosis: None,
        })
    }
}

/// A key that page logic presses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Key {
    Enter,
}

struct KeyCode {
    key: &'static str,
    code: &'static str,
    windows_code: u32,
    text: &'static str,
}

impl Key {
    fn code(self) -> KeyCode {
        match self {
            Self::Enter => KeyCode {
                key: "Enter",
                code: "Enter",
                windows_code: 13,
                text: "\r",
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct Point {
    x: f64,
    y: f64,
}

/// Where an element's box lay in the viewport when a script read it.
#[derive(Clone, Copy, Debug, Deserialize)]
struct Rect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn agrees_with(self, other: Self) -> bool {
        [
            self.left - other.left,
            self.top - other.top,
            self.width - other.width,
            self.height - other.height,
        ]
        .iter()
        .all(|difference| difference.abs() < STEADY_PX)
    }
}

/// How far a target moved from `from` to `to`, in whole CSS pixels.
fn movement(from: Rect, to: Rect) -> String {
    format!(
        "target moved by ({:.0}, {:.0}) px",
        to.left - from.left,
        to.top - from.top
    )
}

/// A point on the one target that a pointer hits, and the target's box when it was read.
#[derive(Clone, Copy, Debug, Deserialize)]
struct Aim {
    x: f64,
    y: f64,
    rect: Rect,
}

impl Aim {
    fn point(self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }
}

#[derive(Deserialize)]
struct WheelTurn {
    x: f64,
    y: f64,
    #[serde(rename = "deltaY")]
    delta_y: f64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Located {
    Scroll { scroll: WheelTurn },
    Reason { reason: String },
    Aim(Aim),
}

/// Why the pointer no longer hits the one target, with the element under the pointer and the
/// target's box now; no reason when the pointer still hits it.
#[derive(Deserialize)]
struct Recheck {
    reason: Option<String>,
    #[serde(default)]
    hit: Option<String>,
    #[serde(default)]
    rect: Option<Rect>,
}

impl Recheck {
    fn diagnosis(&self, located: Rect) -> String {
        let hit = self.hit.as_deref().unwrap_or("nothing");
        let hit: String = hit.chars().take(MAX_HIT_CHARACTERS).collect();
        match self.rect {
            Some(rect) => format!("pointer hit {hit}, {}", movement(located, rect)),
            None => format!("pointer hit {hit}"),
        }
    }
}

fn between((low, high): (f64, f64)) -> f64 {
    fastrand::f64().mul_add(high - low, low)
}

fn millis(range: (f64, f64)) -> Duration {
    Duration::from_secs_f64(between(range) / 1000.0)
}

// Finds the one target among the elements that the page expression `targets` yields. Sites stack
// invisible decoy copies of buttons under the real ones, so an element counts only outside
// `aria-hidden` subtrees, with a size, and when a pointer at its center hits it or a descendant.
// Returns a point on the target that a pointer hits with the target's box, a wheel turn that
// brings it into view, or the reason it cannot. With `at`, it checks that a pointer at `at` still
// hits the one target, and otherwise reports whether the target is gone, has a hittable twin,
// moved, or is covered, with the element under the pointer by tag and class and the target's box.
fn locate_script(targets: &str, at: Option<Aim>) -> String {
    let at = at.map_or(Value::Null, |aim| json!({"x": aim.x, "y": aim.y}));
    format!(
        r#"(() => {{
    const found = [...({targets})].filter((element) => element instanceof Element);
    const center = (rect) => ({{ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }});
    const box = (rect) => ({{ left: rect.left, top: rect.top, width: rect.width, height: rect.height }});
    const hits = (element, x, y) => {{
      const hit = document.elementFromPoint(x, y);
      return Boolean(hit) && element.contains(hit);
    }};
    const candidates = found.filter((element) => {{
      const rect = element.getBoundingClientRect();
      return !element.closest('[aria-hidden="true"]') && rect.width > 0 && rect.height > 0;
    }});
    const hittable = candidates.filter((element) => {{
      const point = center(element.getBoundingClientRect());
      return hits(element, point.x, point.y);
    }});
    const at = {at};
    if (at) {{
      const under = document.elementFromPoint(at.x, at.y);
      const classes = under ? (under.getAttribute('class') || '').trim().split(/\s+/).filter(Boolean) : [];
      const hit = under ? [under.tagName.toLowerCase(), ...classes].join('.').slice(0, {MAX_HIT_CHARACTERS}) : null;
      const target = hittable[0] || candidates[0];
      if (!target) return {{ reason: 'no_target', hit }};
      const rect = target.getBoundingClientRect();
      const report = (reason) => ({{ reason, hit, rect: box(rect) }});
      if (hittable.length > 1) return report('ambiguous_target');
      if (hittable.length === 1) return hits(target, at.x, at.y) ? {{ reason: null }} : report('target_moved');
      const covered = rect.left <= at.x && at.x <= rect.right && rect.top <= at.y && at.y <= rect.bottom;
      return report(covered ? 'obscured_target' : 'target_moved');
    }}
    if (candidates.length === 0) return {{ reason: 'no_target' }};
    if (hittable.length > 1) return {{ reason: 'ambiguous_target' }};
    if (hittable.length === 1) {{
      const rect = hittable[0].getBoundingClientRect();
      const middle = center(rect);
      for (let attempt = 0; attempt < 8; attempt += 1) {{
        const x = middle.x + (Math.random() * 2 - 1) * rect.width * {CLICK_JITTER};
        const y = middle.y + (Math.random() * 2 - 1) * rect.height * {CLICK_JITTER};
        if (hits(hittable[0], x, y)) return {{ x, y, rect: box(rect) }};
      }}
      return {{ ...middle, rect: box(rect) }};
    }}
    const view = {{
      left: 0, top: 0, right: document.documentElement.clientWidth, bottom: document.documentElement.clientHeight,
    }};
    const clip = (area, rect) => ({{
      left: Math.max(area.left, rect.left),
      top: Math.max(area.top, rect.top),
      right: Math.min(area.right, rect.right),
      bottom: Math.min(area.bottom, rect.bottom),
    }});
    const target = candidates[0];
    let visible = view;
    let scroller = null;
    for (let element = target.parentElement; element && element !== document.body; element = element.parentElement) {{
      const style = getComputedStyle(element);
      if (!/(auto|scroll|overlay)/.test(style.overflowY) || element.scrollHeight <= element.clientHeight) continue;
      const rect = element.getBoundingClientRect();
      scroller = scroller || clip(view, rect);
      visible = clip(visible, rect);
    }}
    const point = center(target.getBoundingClientRect());
    if (point.x >= visible.left && point.x <= visible.right && point.y >= visible.top && point.y <= visible.bottom) {{
      return {{ reason: 'obscured_target' }};
    }}
    const wheel = scroller || view;
    if (wheel.right <= wheel.left || wheel.bottom <= wheel.top) return {{ reason: 'out_of_view' }};
    return {{
      scroll: {{
        x: (wheel.left + wheel.right) / 2,
        y: (wheel.top + wheel.bottom) / 2,
        deltaY: point.y - (visible.top + visible.bottom) / 2,
      }},
    }};
  }})()"#
    )
}

impl Page {
    /// Moves the pointer onto the one target that `targets` (a page expression yielding
    /// elements) names, scrolling it into view with the wheel first.
    ///
    /// # Errors
    ///
    /// Returns why no single target could be reached, or a transport failure.
    pub(crate) async fn hover(&mut self, targets: &str) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        let aim = self.locate(targets).await?;
        self.require_time()?;
        self.move_pointer(aim.point()).await
    }

    /// Clicks the one target that `targets` names: scrolls it into view with the wheel, waits
    /// until it stands still, hovers a point near its center that hits it, and clicks there once
    /// the pointer still hits it after the hover. When the hover finds the target moved or
    /// covered, it aims again, at most twice; it presses at most once. Callers confirm the
    /// click's effect with [`Page::confirm`].
    ///
    /// # Errors
    ///
    /// Returns why the click could not be made, with what the pointer hit when the recheck
    /// stopped it, or a transport failure.
    pub(crate) async fn click(&mut self, targets: &str) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        let mut reaims = 0;
        let point = loop {
            let aim = self.locate(targets).await?;
            self.require_time()?;
            self.move_pointer(aim.point()).await?;
            self.rest(millis(HOVER_DWELL_MS)).await;
            let recheck: Recheck = self.evaluate(&locate_script(targets, Some(aim))).await?;
            let Some(reason) = &recheck.reason else {
                break aim.point();
            };
            let failure = InputFailure::from_reason(reason);
            let movable = matches!(
                failure,
                InputFailure::TargetMoved | InputFailure::ObscuredTarget
            );
            if movable && reaims < MAX_REAIMS {
                reaims += 1;
                continue;
            }
            return Err(InputError::Failed(Missed {
                failure,
                diagnosis: Some(recheck.diagnosis(aim.rect)),
            }));
        };
        self.require_time()?;
        self.mouse("mousePressed", point, 1).await?;
        self.rest(millis(PRESS_MS)).await;
        self.mouse("mouseReleased", point, 0).await?;
        Ok(())
    }

    /// Polls the read-only page `check`, which yields a boolean, until it holds or `within`
    /// passes, never past the read deadline. A page that navigates after a click briefly
    /// answers no script, so a failed check counts as not holding yet.
    ///
    /// # Errors
    ///
    /// Returns [`InputFailure::ClickMissed`] when it never holds, or a transport failure once
    /// the browser has closed the connection.
    pub(crate) async fn confirm(&self, check: &str, within: Duration) -> Result<(), InputError> {
        let until = crate::types::Deadline::new(within);
        loop {
            match self.evaluate::<bool>(check).await {
                Ok(true) => return Ok(()),
                Err(failure) if self.closed() => return Err(InputError::Transport(failure)),
                Ok(false) | Err(_) => {}
            }
            if until.remaining().is_none_or(|left| left.is_zero()) || !self.reading() {
                return Err(InputFailure::ClickMissed.into());
            }
            self.rest(LOCATE_POLL.min(until.remaining().unwrap_or_default()))
                .await;
        }
    }

    /// Scrolls by `delta_y` CSS pixels with wheel turns over `at`, which scroll the innermost
    /// scrolling area under it.
    ///
    /// # Errors
    ///
    /// Returns [`InputFailure::DeadlinePassed`] once the read deadline passes, or a transport
    /// failure.
    pub(crate) async fn wheel(&mut self, at: (f64, f64), delta_y: f64) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        self.wheel_at(Point { x: at.0, y: at.1 }, delta_y).await
    }

    /// Types `text` into the focused element in the pieces [`typing_pieces`] cuts it into: each
    /// piece one `Input.insertText`, 30–90 ms after a single character and 100–300 ms before a
    /// group an input method commits.
    ///
    /// # Errors
    ///
    /// Returns [`InputFailure::DeadlinePassed`] once the read deadline passes, or a transport
    /// failure.
    pub(crate) async fn type_text(&mut self, text: &str) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        let pieces = typing_pieces(text, || {
            fastrand::usize(COMMIT_CHARACTERS.0..=COMMIT_CHARACTERS.1)
        });
        for (index, piece) in pieces.iter().enumerate() {
            if index > 0 {
                let gap = if piece.committed {
                    COMMIT_GAP_MS
                } else {
                    KEY_GAP_MS
                };
                self.rest(millis(gap)).await;
            }
            self.require_time()?;
            self.call("Input.insertText", json!({"text": piece.text}))
                .await?;
        }
        Ok(())
    }

    /// Presses `key` in the focused element, with its key code and the text it types.
    ///
    /// # Errors
    ///
    /// Returns [`InputFailure::DeadlinePassed`] once the read deadline passes, or a transport
    /// failure.
    pub(crate) async fn press(&mut self, key: Key) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        self.require_time()?;
        let code = key.code();
        self.key_event(json!({
            "type": "keyDown", "key": code.key, "code": code.code,
            "windowsVirtualKeyCode": code.windows_code, "text": code.text,
        }))
        .await?;
        self.key_event(json!({
            "type": "keyUp", "key": code.key, "code": code.code,
            "windowsVirtualKeyCode": code.windows_code,
        }))
        .await?;
        Ok(())
    }
}

impl Page {
    fn require_time(&self) -> Result<(), InputError> {
        if self.reading() {
            Ok(())
        } else {
            Err(InputFailure::DeadlinePassed.into())
        }
    }

    async fn pause(&self) {
        self.rest(millis(INPUT_PAUSE_MS)).await;
    }

    /// Finds the one target, scrolling it into view, and returns a point on it once two readings
    /// of its box [`STEADY_GAP`] apart agree. A target that is still moving when
    /// [`LOCATE_WITHIN`] has passed since it was first found is [`InputFailure::TargetMoved`].
    async fn locate(&mut self, targets: &str) -> Result<Aim, InputError> {
        let looking = crate::types::Deadline::new(LOCATE_WITHIN);
        let mut scrolls = 0;
        let mut steadying: Option<(Aim, crate::types::Deadline)> = None;
        loop {
            match self
                .evaluate::<Located>(&locate_script(targets, None))
                .await?
            {
                Located::Aim(aim) => {
                    let steady = match steadying {
                        Some((previous, _)) if aim.rect.agrees_with(previous.rect) => {
                            return Ok(aim);
                        }
                        Some((previous, steady)) => {
                            if steady.remaining().is_none_or(|left| left.is_zero())
                                || !self.reading()
                            {
                                return Err(InputError::Failed(Missed {
                                    failure: InputFailure::TargetMoved,
                                    diagnosis: Some(format!(
                                        "{} within {} ms of locating it",
                                        movement(previous.rect, aim.rect),
                                        STEADY_GAP.as_millis()
                                    )),
                                }));
                            }
                            steady
                        }
                        None => crate::types::Deadline::new(LOCATE_WITHIN),
                    };
                    steadying = Some((aim, steady));
                    self.rest(STEADY_GAP).await;
                }
                Located::Scroll { scroll } => {
                    steadying = None;
                    if scrolls >= MAX_SCROLLS_INTO_VIEW {
                        return Err(InputFailure::OutOfView.into());
                    }
                    scrolls += 1;
                    self.wheel_at(
                        Point {
                            x: scroll.x,
                            y: scroll.y,
                        },
                        scroll.delta_y,
                    )
                    .await?;
                }
                Located::Reason { reason } => {
                    steadying = None;
                    let failure = InputFailure::from_reason(&reason);
                    let waited = looking.remaining().is_none_or(|left| left.is_zero());
                    if failure == InputFailure::AmbiguousTarget || waited || !self.reading() {
                        return Err(failure.into());
                    }
                    self.rest(LOCATE_POLL).await;
                }
            }
        }
    }

    async fn wheel_at(&mut self, at: Point, delta_y: f64) -> Result<(), InputError> {
        self.require_time()?;
        self.move_pointer(at).await?;
        for notch in wheel_notches(delta_y) {
            self.rest(notch.wait).await;
            self.require_time()?;
            self.call(
                "Input.dispatchMouseEvent",
                json!({"type": "mouseWheel", "x": at.x, "y": at.y, "deltaX": 0, "deltaY": notch.delta}),
            )
            .await?;
        }
        self.settle_scroll(at).await
    }

    /// Waits until the scrolling areas under `at` stop moving, or [`SCROLL_SETTLE_WITHIN`]
    /// passes; a page still scrolling after that is left to the click's recheck.
    async fn settle_scroll(&self, at: Point) -> Result<(), InputError> {
        let script = format!(
            "(() => {{ let offset = 0; for (let element = document.elementFromPoint({}, {}); element; element = element.parentElement) offset += element.scrollTop; return offset; }})()",
            at.x, at.y
        );
        let settling = crate::types::Deadline::new(SCROLL_SETTLE_WITHIN);
        let mut last: Option<f64> = None;
        while settling.remaining().is_some_and(|left| !left.is_zero()) && self.reading() {
            self.rest(SCROLL_SETTLE_POLL).await;
            let offset: f64 = self.evaluate(&script).await?;
            if last.is_some_and(|previous| (previous - offset).abs() < 0.5) {
                break;
            }
            last = Some(offset);
        }
        Ok(())
    }

    /// Moves the pointer to `to` along a slight curve in several steps, and stops before the
    /// first step past the read deadline.
    async fn move_pointer(&mut self, to: Point) -> Result<(), InputError> {
        let from = self.pointer;
        let control = (
            f64::midpoint(from.0, to.x) + between((-POINTER_CURVE_PX, POINTER_CURVE_PX)),
            f64::midpoint(from.1, to.y) + between((-POINTER_CURVE_PX, POINTER_CURVE_PX)),
        );
        let steps = fastrand::usize(POINTER_STEPS.0..=POINTER_STEPS.1);
        for step in 1..=steps {
            let t = step_fraction(step, steps);
            let along = |start: f64, middle: f64, end: f64| {
                (1.0 - t).powi(2) * start + 2.0 * (1.0 - t) * t * middle + t * t * end
            };
            let (x, y) = if step == steps {
                (to.x, to.y)
            } else {
                (
                    along(from.0, control.0, to.x),
                    along(from.1, control.1, to.y),
                )
            };
            self.require_time()?;
            self.call(
                "Input.dispatchMouseEvent",
                json!({"type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": 0}),
            )
            .await?;
            self.pointer = (x, y);
            if step < steps {
                self.rest(millis(POINTER_STEP_MS)).await;
            }
        }
        Ok(())
    }

    async fn mouse(&self, kind: &str, at: Point, buttons: u32) -> Result<(), InputError> {
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": at.x, "y": at.y, "button": "left", "buttons": buttons, "clickCount": 1}),
        )
        .await?;
        Ok(())
    }

    async fn key_event(&self, event: Value) -> Result<(), InputError> {
        self.call("Input.dispatchKeyEvent", event).await?;
        Ok(())
    }
}

/// One piece of typed text, and whether an input method commits it as a group.
#[derive(Debug, Eq, PartialEq)]
struct Piece<'a> {
    text: &'a str,
    committed: bool,
}

/// Cuts `text` into the pieces it is typed in: runs of CJK characters in groups of the sizes
/// `group` picks, as an input method commits them, and every other character alone. Text over
/// 200 characters is one piece, so that it fits the command's time.
fn typing_pieces(text: &str, mut group: impl FnMut() -> usize) -> Vec<Piece<'_>> {
    if text.chars().count() > MAX_TYPED_CHARACTERS {
        return vec![Piece {
            text,
            committed: false,
        }];
    }
    let mut pieces = Vec::new();
    let mut characters = text.char_indices().peekable();
    while let Some((start, character)) = characters.next() {
        let mut end = start + character.len_utf8();
        let committed = is_cjk(character);
        if committed {
            for _ in 1..group().max(1) {
                let Some((index, next)) = characters.next_if(|(_, next)| is_cjk(*next)) else {
                    break;
                };
                end = index + next.len_utf8();
            }
        }
        pieces.push(Piece {
            text: &text[start..end],
            committed,
        });
    }
    pieces
}

/// Whether an input method composes `character`: Han ideographs, kana, and Hangul syllables.
/// Punctuation, even full-width, is typed alone.
fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3040}'..='\u{30FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{3134F}'
    )
}

/// One wheel notch: the wait before it and how far it scrolls.
#[derive(Debug)]
struct Notch {
    wait: Duration,
    delta: f64,
}

/// The notches that scroll `delta_y` CSS pixels: 80–120 px each (the last one what is left),
/// 30–90 ms apart in bursts of 3–8 with a 200–700 ms pause between bursts, and at most
/// [`MAX_WHEEL_NOTCHES`]. The first notch does not wait.
fn wheel_notches(delta_y: f64) -> Vec<Notch> {
    let mut notches = Vec::new();
    let mut remaining = delta_y;
    let mut burst = 0;
    while remaining.abs() >= 1.0 && notches.len() < MAX_WHEEL_NOTCHES {
        let wait = if notches.is_empty() {
            Duration::ZERO
        } else if burst == 0 {
            millis(WHEEL_BURST_PAUSE_MS)
        } else {
            millis(WHEEL_GAP_MS)
        };
        if burst == 0 {
            burst = fastrand::usize(WHEEL_BURST.0..=WHEEL_BURST.1);
        }
        burst -= 1;
        let delta = remaining.signum() * remaining.abs().min(between(WHEEL_NOTCH_PX));
        remaining -= delta;
        notches.push(Notch { wait, delta });
    }
    notches
}

fn step_fraction(step: usize, steps: usize) -> f64 {
    let step = u32::try_from(step).unwrap_or(u32::MAX);
    let steps = u32::try_from(steps).unwrap_or(u32::MAX).max(1);
    f64::from(step) / f64::from(steps)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
    use tokio::time::Instant;

    use super::super::capture::Capture;
    use super::super::{CaptureLevel, Deadlines, Page};
    use super::{
        InputError, InputFailure, Missed, Piece, Point, Recheck, Rect, typing_pieces, wheel_notches,
    };
    use crate::providers::cdp::Cdp;
    use crate::types::Deadline;

    /// The methods a browser received, each with when it arrived.
    type Received = Arc<Mutex<Vec<(String, Instant)>>>;

    /// A page whose browser answers every command at once, with `read` left before its read
    /// deadline.
    fn page(read: Duration) -> (Page, Received) {
        let (writer, mut commands) = duplex(1 << 20);
        let (mut answers, reader) = duplex(1 << 20);
        let (cdp, events) = Cdp::start(reader, writer);
        let received = Received::default();
        let log = Arc::clone(&received);
        tokio::spawn(async move {
            let mut message = Vec::new();
            while let Ok(byte) = commands.read_u8().await {
                if byte != 0 {
                    message.push(byte);
                    continue;
                }
                let command: Value = serde_json::from_slice(&message).expect("command is JSON");
                message.clear();
                log.lock().expect("log").push((
                    command["method"].as_str().unwrap_or_default().to_owned(),
                    Instant::now(),
                ));
                let mut answer = serde_json::to_vec(&json!({"id": command["id"], "result": {}}))
                    .expect("encode answer");
                answer.push(0);
                answers.write_all(&answer).await.expect("answer");
            }
        });
        let deadlines = Deadlines {
            work: Deadline::new(Duration::from_secs(10)),
            read: Deadline::new(read),
        };
        let capture = Capture::start(
            cdp.clone(),
            "session".into(),
            events,
            |_| CaptureLevel::Ignore,
            deadlines.work,
        );
        let page = Page {
            cdp,
            session: "session".into(),
            deadlines,
            capture,
            pointer: (0.0, 0.0),
        };
        (page, received)
    }

    // Paused time advances only while every task waits on a timer, so each step lands at an
    // exact instant.
    fn run<T>(test: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .expect("build test runtime")
            .block_on(test)
    }

    #[test]
    fn a_pointer_move_that_the_read_deadline_interrupts_sends_no_later_step() {
        let read = Duration::from_millis(30);

        let (moved, started, received) = run(async {
            let started = Instant::now();
            let (mut page, received) = page(read);
            let moved = page.move_pointer(Point { x: 400.0, y: 300.0 }).await;
            (moved, started, received)
        });

        let late = received
            .lock()
            .expect("log")
            .iter()
            .filter(|(method, at)| {
                method == "Input.dispatchMouseEvent" && at.duration_since(started) >= read
            })
            .count();
        assert!(
            matches!(
                (moved, late),
                (
                    Err(InputError::Failed(Missed {
                        failure: InputFailure::DeadlinePassed,
                        diagnosis: None
                    })),
                    0
                )
            ),
            "{late} pointer steps went out after the read deadline"
        );
    }

    #[test]
    fn a_recheck_diagnosis_keeps_at_most_80_characters_of_the_element_under_the_pointer() {
        let located = Rect {
            left: 100.0,
            top: 100.0,
            width: 120.0,
            height: 40.0,
        };
        let recheck = |rect| Recheck {
            reason: Some("target_moved".into()),
            hit: Some(format!("div.{}", "a".repeat(100))),
            rect,
        };

        let diagnoses = [
            None,
            Some(Rect {
                top: 60.4,
                ..located
            }),
        ]
        .map(|rect| recheck(rect).diagnosis(located));

        let hit = format!("div.{}", "a".repeat(76));
        assert_eq!(
            diagnoses,
            [
                format!("pointer hit {hit}"),
                format!("pointer hit {hit}, target moved by (0, -40) px"),
            ]
        );
    }

    /// The pieces of `text` and whether each is a committed group, with group sizes taken in
    /// turn from `sizes`.
    fn pieces<'a>(text: &'a str, sizes: &[usize]) -> Vec<(&'a str, bool)> {
        let mut sizes = sizes.iter().copied().cycle();
        typing_pieces(text, || sizes.next().unwrap_or(1))
            .into_iter()
            .map(|Piece { text, committed }| (text, committed))
            .collect()
    }

    #[test]
    fn chinese_text_is_committed_in_the_groups_an_input_method_picks() {
        assert_eq!(
            pieces("咖啡拿铁推荐", &[2, 1, 3]),
            [("咖啡", true), ("拿", true), ("铁推荐", true)]
        );
    }

    #[test]
    fn latin_letters_and_digits_beside_chinese_are_typed_one_at_a_time() {
        assert_eq!(
            pieces("Mac咖啡2", &[3]),
            [
                ("M", false),
                ("a", false),
                ("c", false),
                ("咖啡", true),
                ("2", false)
            ]
        );
    }

    #[test]
    fn punctuation_ends_a_chinese_group_and_is_typed_alone() {
        assert_eq!(
            pieces("咖啡，拿铁 推荐", &[3]),
            [
                ("咖啡", true),
                ("，", false),
                ("拿铁", true),
                (" ", false),
                ("推荐", true)
            ]
        );
    }

    #[test]
    fn text_over_200_characters_is_one_insertion() {
        let typed = |length| pieces(&"咖".repeat(length), &[2]).len();

        assert_eq!([typed(200), typed(201)], [100, 1]);
    }

    #[test]
    fn a_wheel_turn_goes_in_bursts_of_three_to_eight_small_notches() {
        fastrand::seed(274);
        for distance in [5000.0, -5000.0, 750.0] {
            for _ in 0..200 {
                let notches = wheel_notches(distance);
                let scrolled: f64 = notches.iter().map(|notch| notch.delta).sum();
                let (last, rest) = notches.split_last().expect("at least one notch");
                let mut bursts = vec![0_usize];
                for notch in &notches[1..] {
                    let wait = notch.wait.as_secs_f64() * 1000.0;
                    if (200.0..=700.0).contains(&wait) {
                        bursts.push(0);
                    } else {
                        assert!((30.0..=90.0).contains(&wait), "{notches:?}");
                    }
                    *bursts.last_mut().expect("a burst") += 1;
                }
                *bursts.first_mut().expect("a burst") += 1;
                let (last_burst, full_bursts) = bursts.split_last().expect("a burst");
                assert!(
                    notches.len() <= 12
                        && notches[0].wait.is_zero()
                        && rest
                            .iter()
                            .all(|notch| (80.0..=120.0).contains(&notch.delta.abs()))
                        && last.delta.abs() <= 120.0
                        && notches
                            .iter()
                            .all(|notch| notch.delta.signum() == distance.signum())
                        && full_bursts.iter().all(|burst| (3..=8).contains(burst))
                        && *last_burst <= 8
                        && (notches.len() == 12 || (scrolled - distance).abs() < 1.0),
                    "{notches:?}"
                );
            }
        }
    }
}
