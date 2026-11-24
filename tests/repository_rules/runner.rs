//! The runner configuration: no profile retries a failed test, and every override, which is an
//! entry of the exemption list, says why its tests may wait.

/// The rule breaks in `config`, the text of the runner configuration, each with its line.
pub(crate) fn runner_problems(config: &str) -> Vec<String> {
    let lines = config.lines().map(str::trim).collect::<Vec<_>>();
    let mut problems = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        if line.starts_with("retries") {
            problems.push(format!("line {number}: a profile retries failed tests"));
        }
        if !(line.starts_with("[[profile.") && line.ends_with(".overrides]]")) {
            continue;
        }
        let reason = lines[..index]
            .iter()
            .rev()
            .take_while(|line| line.starts_with('#'))
            .collect::<Vec<_>>();
        if reason.is_empty() {
            problems.push(format!(
                "line {number}: an exemption without a reason in the comment above it"
            ));
        }
        let entry = lines[index + 1..]
            .iter()
            .take_while(|line| !line.starts_with('['))
            .copied()
            .collect::<String>();
        let smoke_or_doctor = ["test(=smoke::", "test(=doctor::"]
            .iter()
            .any(|module| entry.contains(module));
        // Spec 5 removes smoke and replaces doctor's probes; the mark makes their entries leave
        // with them instead of outliving the tests they excuse.
        if smoke_or_doctor && !reason.iter().any(|line| line.contains("spec 5")) {
            problems.push(format!(
                "line {number}: a smoke or doctor exemption not marked as handled by spec 5"
            ));
        }
    }
    problems
}

/// The limits in `config` that the shared test environment's command deadline cannot read, each
/// naming its profile and override. It reads an override's filter only as one `binary_id`, or as
/// exact test names behind an optional `binary_id`, and every limit only as whole seconds; any
/// other form would leave a test with the default deadline while the runner applies another.
pub(crate) fn unread_limits(config: &str) -> Vec<String> {
    let Ok(config) = config.parse::<toml::Table>() else {
        return vec!["the runner configuration is not TOML".to_owned()];
    };
    let Some(profiles) = config.get("profile").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut problems = Vec::new();
    for (name, profile) in profiles {
        if let Some(timeout) = profile.get("slow-timeout")
            && !is_whole_seconds(timeout)
        {
            problems.push(format!(
                "profile.{name}: slow-timeout {timeout} is not whole seconds"
            ));
        }
        let overrides = profile.get("overrides").and_then(toml::Value::as_array);
        for (index, entry) in overrides.into_iter().flatten().enumerate() {
            let label = format!("profile.{name} override {}", index + 1);
            let filter = entry
                .get("filter")
                .and_then(toml::Value::as_str)
                .unwrap_or_default();
            if !names_exact_tests_or_a_binary(filter) {
                let filter = filter.split_whitespace().collect::<Vec<_>>().join(" ");
                problems.push(format!(
                    "{label}: filter `{filter}` is not exact test names or one binary"
                ));
            }
            match entry.get("slow-timeout") {
                Some(timeout) if is_whole_seconds(timeout) => {}
                Some(timeout) => {
                    problems.push(format!(
                        "{label}: slow-timeout {timeout} is not whole seconds"
                    ));
                }
                None => problems.push(format!("{label}: sets no slow-timeout")),
            }
        }
    }
    problems
}

fn names_exact_tests_or_a_binary(filter: &str) -> bool {
    if argument(filter, "binary_id(").is_some() {
        return true;
    }
    let filter = filter.split_whitespace().collect::<String>();
    let tests = match filter.split_once('&') {
        Some((binary, tests)) => argument(binary, "binary_id(").map(|_| tests),
        None => Some(filter.as_str()),
    };
    tests.is_some_and(|tests| {
        let tests = tests
            .strip_prefix('(')
            .and_then(|tests| tests.strip_suffix(')'))
            .unwrap_or(tests);
        tests
            .split('|')
            .all(|test| argument(test, "test(=").is_some())
    })
}

/// The argument of `atom` when it is one call of `function`, which names its opening parenthesis.
fn argument<'a>(atom: &'a str, function: &str) -> Option<&'a str> {
    atom.strip_prefix(function)?
        .strip_suffix(')')
        .filter(|argument| {
            !argument.is_empty()
                && !argument.contains(|c: char| "()&|".contains(c) || c.is_whitespace())
        })
}

fn is_whole_seconds(timeout: &toml::Value) -> bool {
    let period = timeout
        .get("period")
        .and_then(toml::Value::as_str)
        .and_then(|period| period.strip_suffix('s'))
        .is_some_and(|seconds| seconds.parse::<u64>().is_ok());
    let terminate_after = timeout
        .get("terminate-after")
        .and_then(toml::Value::as_integer)
        .is_some_and(|count| u32::try_from(count).is_ok());
    period && terminate_after
}
