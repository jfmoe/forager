//! The pacing of native input: the random pauses around actions, the pieces text is typed in,
//! and the notches a wheel turn is made of. Pure decisions; `browser_input` sends the events.

use std::time::Duration;

pub(super) const INPUT_PAUSE_MS: (f64, f64) = (300.0, 1500.0);
pub(super) const HOVER_DWELL_MS: (f64, f64) = (80.0, 250.0);
pub(super) const PRESS_MS: (f64, f64) = (50.0, 130.0);
pub(super) const KEY_GAP_MS: (f64, f64) = (30.0, 90.0);
// An input method commits a few characters at a time, after the user picks them.
pub(super) const COMMIT_GAP_MS: (f64, f64) = (100.0, 300.0);
pub(super) const COMMIT_CHARACTERS: (usize, usize) = (1, 3);
const MAX_TYPED_CHARACTERS: usize = 200;
// A mouse wheel turns in notches of about 100 px, a few at a time with a pause between bursts.
const WHEEL_NOTCH_PX: (f64, f64) = (80.0, 120.0);
const WHEEL_GAP_MS: (f64, f64) = (30.0, 90.0);
const WHEEL_BURST: (usize, usize) = (3, 8);
const WHEEL_BURST_PAUSE_MS: (f64, f64) = (200.0, 700.0);
// At the slowest pacing 12 notches take 2.8 seconds, which keeps five pages of search results
// or comments within the default attempt timeout when each page takes one wheel turn.
const MAX_WHEEL_NOTCHES: usize = 12;

pub(super) fn between((low, high): (f64, f64)) -> f64 {
    fastrand::f64().mul_add(high - low, low)
}

pub(super) fn millis(range: (f64, f64)) -> Duration {
    Duration::from_secs_f64(between(range) / 1000.0)
}

/// One piece of typed text, and whether an input method commits it as a group.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Piece<'a> {
    pub(super) text: &'a str,
    pub(super) committed: bool,
}

/// Cuts `text` into the pieces it is typed in: runs of CJK characters in groups of the sizes
/// `group` picks, as an input method commits them, and every other character alone. Text over
/// 200 characters is one piece, so that it fits the command's time.
pub(super) fn typing_pieces(text: &str, mut group: impl FnMut() -> usize) -> Vec<Piece<'_>> {
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
pub(super) struct Notch {
    pub(super) wait: Duration,
    pub(super) delta: f64,
}

/// The notches that scroll `delta_y` CSS pixels: 80–120 px each (the last one what is left),
/// 30–90 ms apart in bursts of 3–8 with a 200–700 ms pause between bursts, and at most
/// [`MAX_WHEEL_NOTCHES`]. The first notch does not wait.
pub(super) fn wheel_notches(delta_y: f64) -> Vec<Notch> {
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

#[cfg(test)]
mod tests {
    use super::{Piece, typing_pieces, wheel_notches};

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

    #[test]
    fn five_pages_at_the_slowest_input_pacing_fit_the_default_route_timeout() {
        use std::time::Duration;

        use super::{
            INPUT_PAUSE_MS, MAX_WHEEL_NOTCHES, WHEEL_BURST, WHEEL_BURST_PAUSE_MS, WHEEL_GAP_MS,
        };
        use crate::config::BrowserRoute;
        use crate::providers::browser::input::{
            POINTER_STEP_MS, POINTER_STEPS, SCROLL_SETTLE_WITHIN,
        };
        use crate::providers::browser::{CLEANUP_RESERVE, READ_RESERVE};

        const PAGES: u32 = 5;
        // The time this budget allows for each page's response to arrive.
        const PAGE_RESPONSE: Duration = Duration::from_secs(2);
        let ms = |millis: f64| Duration::from_secs_f64(millis / 1000.0);

        let bursts = MAX_WHEEL_NOTCHES.div_ceil(WHEEL_BURST.0);
        let burst_pauses = u32::try_from(bursts - 1).expect("burst count");
        let notch_gaps = u32::try_from(MAX_WHEEL_NOTCHES - 1).expect("notch count") - burst_pauses;
        let wheel = ms(WHEEL_BURST_PAUSE_MS.1) * burst_pauses + ms(WHEEL_GAP_MS.1) * notch_gaps;
        let pointer = ms(POINTER_STEP_MS.1) * u32::try_from(POINTER_STEPS.1).expect("steps");
        let scroll = ms(INPUT_PAUSE_MS.1) + pointer + wheel + SCROLL_SETTLE_WITHIN;
        let read = (scroll + PAGE_RESPONSE) * PAGES;
        let budget = Duration::from_secs(BrowserRoute::default().timeout)
            .checked_sub(CLEANUP_RESERVE + READ_RESERVE)
            .expect("the default timeout exceeds the reserves");

        assert!(
            read <= budget,
            "{PAGES} pages at the slowest pacing take {read:?}, past the {budget:?} a default browser command has to read; shorten the pacing or raise the default timeout"
        );
    }
}
