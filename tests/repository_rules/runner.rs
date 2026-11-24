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
