//! Native input: every action that changes the page (pointer moves and hovers, clicks, wheel
//! scrolls, keys, and text) goes through the browser's input pipeline as CDP `Input.*` events, so
//! the page sees trusted events with coordinates and a full pointer sequence. Nothing here calls
//! `element.click()`, `dispatchEvent`, `execCommand`, or assigns or scrolls a scroll position
//! from script; reading the page to find a target is not an action.
//!
//! The pacing is a person's: a random pause of 0.3–1.5 seconds before each action, a hover
//! before each click, a pointer that moves to its target in several steps, and short text typed
//! one character at a time. No input goes out after the read deadline. An action that cannot be
//! done natively fails with its reason and never falls back to a scripted one.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::Page;
use crate::net::AttemptFailure;

const INPUT_PAUSE_MS: (f64, f64) = (300.0, 1500.0);
const HOVER_DWELL_MS: (f64, f64) = (80.0, 250.0);
const PRESS_MS: (f64, f64) = (50.0, 130.0);
const KEY_GAP_MS: (f64, f64) = (30.0, 90.0);
const WHEEL_GAP_MS: (f64, f64) = (30.0, 90.0);
const WHEEL_STEP_PX: (f64, f64) = (300.0, 500.0);
const MAX_WHEEL_STEPS: usize = 12;
const POINTER_STEPS: (usize, usize) = (8, 18);
const POINTER_STEP_MS: (f64, f64) = (8.0, 22.0);
const POINTER_CURVE_PX: f64 = 60.0;
const MAX_TYPED_CHARACTERS: usize = 200;
// A target may render a moment after the action that reveals it.
const LOCATE_WITHIN: Duration = Duration::from_secs(2);
const LOCATE_POLL: Duration = Duration::from_millis(200);
const MAX_SCROLLS_INTO_VIEW: usize = 8;
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

/// A native action that failed: on the page, or in the browser connection.
#[derive(Debug)]
pub(crate) enum InputError {
    Failed(InputFailure),
    Transport(AttemptFailure),
}

impl From<AttemptFailure> for InputError {
    fn from(failure: AttemptFailure) -> Self {
        Self::Transport(failure)
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
    Point(Point),
}

/// Why the pointer no longer hits the one target; no reason when it still does.
#[derive(Deserialize)]
struct Recheck {
    reason: Option<String>,
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
// Returns a point on the target that a pointer hits, a wheel turn that brings it into view, or
// the reason it cannot. With `at`, it checks that a pointer at `at` still hits the one target,
// and otherwise reports whether the target is gone, has a hittable twin, or moved.
fn locate_script(targets: &str, at: Option<Point>) -> String {
    let at = at.map_or(Value::Null, |point| json!({"x": point.x, "y": point.y}));
    format!(
        r#"(() => {{
    const found = [...({targets})].filter((element) => element instanceof Element);
    const center = (rect) => ({{ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }});
    const hits = (element, x, y) => {{
      const hit = document.elementFromPoint(x, y);
      return Boolean(hit) && element.contains(hit);
    }};
    const candidates = found.filter((element) => {{
      const rect = element.getBoundingClientRect();
      return !element.closest('[aria-hidden="true"]') && rect.width > 0 && rect.height > 0;
    }});
    const at = {at};
    if (candidates.length === 0) return {{ reason: 'no_target' }};
    const hittable = candidates.filter((element) => {{
      const point = center(element.getBoundingClientRect());
      return hits(element, point.x, point.y);
    }});
    if (hittable.length > 1) return {{ reason: 'ambiguous_target' }};
    if (at) return hittable.length === 1 && hits(hittable[0], at.x, at.y) ? {{ reason: null }} : {{ reason: 'target_moved' }};
    if (hittable.length === 1) {{
      const rect = hittable[0].getBoundingClientRect();
      const middle = center(rect);
      for (let attempt = 0; attempt < 8; attempt += 1) {{
        const x = middle.x + (Math.random() * 2 - 1) * rect.width * {CLICK_JITTER};
        const y = middle.y + (Math.random() * 2 - 1) * rect.height * {CLICK_JITTER};
        if (hits(hittable[0], x, y)) return {{ x, y }};
      }}
      return middle;
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
        let point = self.locate(targets).await?;
        self.require_time()?;
        self.move_pointer(point).await
    }

    /// Clicks the one target that `targets` names: scrolls it into view with the wheel, hovers a
    /// point near its center that hits it, and clicks there once the pointer still hits it after
    /// the hover. Callers confirm the click's effect with [`Page::confirm`].
    ///
    /// # Errors
    ///
    /// Returns why the click could not be made, or a transport failure.
    pub(crate) async fn click(&mut self, targets: &str) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        let point = self.locate(targets).await?;
        self.require_time()?;
        self.move_pointer(point).await?;
        self.rest(millis(HOVER_DWELL_MS)).await;
        let recheck: Recheck = self.evaluate(&locate_script(targets, Some(point))).await?;
        if let Some(reason) = recheck.reason {
            return Err(InputError::Failed(InputFailure::from_reason(&reason)));
        }
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
                return Err(InputError::Failed(InputFailure::ClickMissed));
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

    /// Types `text` into the focused element: up to 200 characters one at a time, 30–90 ms
    /// apart; longer text in one insertion, so that it fits the command's time.
    ///
    /// # Errors
    ///
    /// Returns [`InputFailure::DeadlinePassed`] once the read deadline passes, or a transport
    /// failure.
    pub(crate) async fn type_text(&mut self, text: &str) -> Result<(), InputError> {
        self.require_time()?;
        self.pause().await;
        if text.chars().count() > MAX_TYPED_CHARACTERS {
            self.require_time()?;
            self.call("Input.insertText", json!({"text": text})).await?;
            return Ok(());
        }
        for (index, character) in text.chars().enumerate() {
            if index > 0 {
                self.rest(millis(KEY_GAP_MS)).await;
            }
            self.require_time()?;
            self.call("Input.insertText", json!({"text": character.to_string()}))
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
            Err(InputError::Failed(InputFailure::DeadlinePassed))
        }
    }

    async fn pause(&self) {
        self.rest(millis(INPUT_PAUSE_MS)).await;
    }

    async fn locate(&mut self, targets: &str) -> Result<Point, InputError> {
        let looking = crate::types::Deadline::new(LOCATE_WITHIN);
        let mut scrolls = 0;
        loop {
            match self
                .evaluate::<Located>(&locate_script(targets, None))
                .await?
            {
                Located::Point(point) => return Ok(point),
                Located::Scroll { scroll } => {
                    if scrolls >= MAX_SCROLLS_INTO_VIEW {
                        return Err(InputError::Failed(InputFailure::OutOfView));
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
                    let failure = InputFailure::from_reason(&reason);
                    let waited = looking.remaining().is_none_or(|left| left.is_zero());
                    if failure == InputFailure::AmbiguousTarget || waited || !self.reading() {
                        return Err(InputError::Failed(failure));
                    }
                    self.rest(LOCATE_POLL).await;
                }
            }
        }
    }

    async fn wheel_at(&mut self, at: Point, delta_y: f64) -> Result<(), InputError> {
        self.require_time()?;
        self.move_pointer(at).await?;
        let mut remaining = delta_y;
        for step in 0..MAX_WHEEL_STEPS {
            if remaining.abs() < 1.0 {
                break;
            }
            if step > 0 {
                self.rest(millis(WHEEL_GAP_MS)).await;
            }
            self.require_time()?;
            let delta = remaining.signum() * remaining.abs().min(between(WHEEL_STEP_PX));
            self.call(
                "Input.dispatchMouseEvent",
                json!({"type": "mouseWheel", "x": at.x, "y": at.y, "deltaX": 0, "deltaY": delta}),
            )
            .await?;
            remaining -= delta;
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
    use super::{InputError, InputFailure, Point};
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
                (Err(InputError::Failed(InputFailure::DeadlinePassed)), 0)
            ),
            "{late} pointer steps went out after the read deadline"
        );
    }
}
