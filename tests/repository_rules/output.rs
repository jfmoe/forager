//! Only the CLI layer writes to stdout and stderr, so what a run did reaches the user through
//! the views of the Attempt Trace and never through a print in a lower layer. Test code is
//! exempt.

/// Macros and calls that write to a standard stream.
const WRITES: [&str; 7] = [
    "print!",
    "println!",
    "eprint!",
    "eprintln!",
    "dbg!",
    "stdout()",
    "stderr()",
];

/// The lines of `source` outside its test code that write to a standard stream, numbered from 1.
pub(crate) fn writes_outside_tests(source: &str) -> Vec<(usize, String)> {
    let code = without_test_items(&code_only(source));
    code.lines()
        .zip(source.lines())
        .enumerate()
        .filter(|(_, (code, _))| WRITES.iter().any(|write| contains_word(code, write)))
        .map(|(index, (_, line))| (index + 1, line.trim().to_owned()))
        .collect()
}

/// Whether `text` holds `word` where no identifier character precedes it.
fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        text[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !(before.is_alphanumeric() || before == '_'))
    })
}

/// `source` with every comment, string and character literal blanked to spaces, keeping its
/// line breaks, so only code remains to scan and to match braces in.
fn code_only(source: &str) -> String {
    let characters = source.chars().collect::<Vec<_>>();
    let mut code = String::with_capacity(source.len());
    let mut index = 0;
    let blank = |code: &mut String, character: char| {
        code.push(if character == '\n' { '\n' } else { ' ' });
    };
    while index < characters.len() {
        let rest = &characters[index..];
        let end = match rest {
            ['/', '/', ..] => rest.iter().position(|c| *c == '\n').unwrap_or(rest.len()),
            ['/', '*', ..] => {
                let mut depth = 0;
                let mut at = 0;
                while at < rest.len() {
                    match &rest[at..] {
                        ['/', '*', ..] => {
                            depth += 1;
                            at += 2;
                        }
                        ['*', '/', ..] => {
                            depth -= 1;
                            at += 2;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => at += 1,
                    }
                }
                at
            }
            ['r', '#' | '"', ..] if raw_string_end(rest).is_some() => {
                raw_string_end(rest).unwrap_or(rest.len())
            }
            ['b', '"', ..] => 1 + string_end(&rest[1..]),
            ['"', ..] => string_end(rest),
            ['\'', '\\', ..] => rest
                .iter()
                .skip(2)
                .position(|c| *c == '\'')
                .map_or(rest.len(), |at| at + 3),
            ['\'', _, '\'', ..] => 3,
            _ => 0,
        };
        if end == 0 {
            code.push(characters[index]);
            index += 1;
        } else {
            for character in &rest[..end] {
                blank(&mut code, *character);
            }
            index += end;
        }
    }
    code
}

/// The length of the string literal at the start of `text`, quotes included.
fn string_end(text: &[char]) -> usize {
    let mut at = 1;
    while at < text.len() {
        match text[at] {
            '\\' => at += 2,
            '"' => return at + 1,
            _ => at += 1,
        }
    }
    text.len()
}

/// The length of the raw string literal at the start of `text`, when one starts there.
fn raw_string_end(text: &[char]) -> Option<usize> {
    let hashes = text[1..].iter().take_while(|c| **c == '#').count();
    if text.get(1 + hashes) != Some(&'"') {
        return None;
    }
    let body = 2 + hashes;
    (body..text.len())
        .find(|at| {
            text[*at] == '"'
                && text[at + 1..]
                    .iter()
                    .take(hashes)
                    .filter(|c| **c == '#')
                    .count()
                    == hashes
        })
        .map(|at| at + 1 + hashes)
}

/// `code` with each item that `#[cfg(test)]` marks blanked, through its closing brace or
/// semicolon.
fn without_test_items(code: &str) -> String {
    const MARK: &str = "#[cfg(test)]";
    let mut characters = code.chars().collect::<Vec<_>>();
    let text = code.to_owned();
    for (start, _) in text.match_indices(MARK) {
        let start = text[..start].chars().count();
        let mut depth = 0;
        let mut end = characters.len();
        for (at, character) in characters.iter().enumerate().skip(start + MARK.len()) {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = at + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = at + 1;
                    break;
                }
                _ => {}
            }
        }
        for character in &mut characters[start..end] {
            if *character != '\n' {
                *character = ' ';
            }
        }
    }
    characters.into_iter().collect()
}

#[test]
fn a_write_in_code_is_found_and_one_in_a_test_comment_or_literal_is_not() {
    let source = r##"
fn report() {
    eprintln!("found");
}
// println!("a comment");
const PAGE: &str = r#"console.log("print!(x)")"#;
fn brace() -> char { '{' }
fn read(child: &mut Child) { child.take_stdout(); }
#[cfg(test)]
fn helper() { println!("{"); }
fn after() { std::io::stdout(); }
#[cfg(test)]
mod tests {
    fn shown() { dbg!(1); }
}
"##;

    assert_eq!(
        writes_outside_tests(source),
        [
            (3, "eprintln!(\"found\");".to_owned()),
            (11, "fn after() { std::io::stdout(); }".to_owned()),
        ]
    );
}
