//! The scheduled job files in `scripts/schedules/`: every live run a job starts is unattended
//! and keeps out of the `attended` tier, each scheduled tier runs at the interval it declares,
//! and the weekly record of the GitHub scheduled workflows names each of them. It is a rule test
//! kept in the crate, as the platform checklist is, because which providers have attended checks
//! and how often each tier runs are declared only here.

use std::fs;
use std::path::Path;

use super::declared;
use crate::catalog::LiveTier;

const JOBS: &str = "scripts/schedules";
const WORKFLOWS: &str = ".github/workflows";

/// One file of the scheduled jobs: a script or a launchd job definition.
struct Job {
    name: String,
    body: String,
}

fn jobs(root: &Path) -> Vec<Job> {
    let mut jobs = fs::read_dir(root.join(JOBS))
        .expect("read the scheduled jobs")
        .map(|entry| {
            let path = entry.expect("a job entry").path();
            Job {
                name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("a job name")
                    .to_owned(),
                body: fs::read_to_string(&path).expect("read a job"),
            }
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|left, right| left.name.cmp(&right.name));
    jobs
}

/// The lines of a job file as the shell reads them: a line ending in a backslash goes on in the
/// next, and a comment is left out.
fn logical_lines(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for line in body.lines() {
        let line = line.trim();
        if current.is_empty() && line.starts_with('#') {
            continue;
        }
        if let Some(continued) = line.strip_suffix('\\') {
            current.push_str(continued.trim_end());
            current.push(' ');
        } else {
            current.push_str(line);
            lines.push(std::mem::take(&mut current));
        }
    }
    lines.push(current);
    lines
}

/// The words of a command line as the shell splits them: quotes and a `${…}` expansion keep their
/// spaces inside one word, and the quotes are dropped.
fn words(command: &str) -> impl Iterator<Item = String> {
    let mut words = Vec::new();
    let mut word = None::<String>;
    let (mut quote, mut braces) = (None, 0_usize);
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        match (quote, character) {
            (None, '\'' | '"') if braces == 0 => {
                quote = Some(character);
                word.get_or_insert_default();
            }
            (Some(open), _) if open == character => quote = None,
            (None, '$') if characters.peek() == Some(&'{') => {
                braces += 1;
                word.get_or_insert_default().push(character);
            }
            (None, '}') if braces > 0 => {
                braces -= 1;
                word.get_or_insert_default().push(character);
            }
            (None, _) if character.is_whitespace() && braces == 0 => words.extend(word.take()),
            _ => word.get_or_insert_default().push(character),
        }
    }
    words.extend(word);
    words.into_iter()
}

/// The values of `option` in a command line, given as the next word or after `=`, split at
/// commas.
fn values(command: &str, option: &str) -> Vec<String> {
    let words = words(command).collect::<Vec<_>>();
    let mut values = Vec::new();
    for (at, word) in words.iter().enumerate() {
        if word == option {
            values.extend(words.get(at + 1).map(String::as_str));
        } else if let Some(value) = word
            .strip_prefix(option)
            .and_then(|rest| rest.strip_prefix('='))
        {
            values.push(value);
        }
    }
    values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::to_owned)
        .collect()
}

/// Each `doctor --live` command in a job file, however its options are ordered, written or
/// continued, and whatever path names the binary.
fn live_commands(body: &str) -> Vec<String> {
    logical_lines(body)
        .into_iter()
        .filter(|line| {
            words(line).any(|word| word == "doctor")
                && words(line).any(|word| word == "--live" || word.starts_with("--live="))
        })
        .collect()
}

/// The names in a job file: its runs of letters, digits and underscores, so `--tier=attended`
/// names `attended` and `--unattended` does not.
fn names(body: &str) -> impl Iterator<Item = &str> {
    body.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|name| !name.is_empty())
}

/// What breaks the schedule rules in `jobs`, given the names of the scheduled workflows and
/// of the providers with attended checks.
fn schedule_problems(jobs: &[Job], workflows: &[String], attended: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    for file in jobs {
        for command in live_commands(&file.body) {
            if !words(&command).any(|word| word == "--unattended") {
                problems.push(format!("{}: `{command}` lacks --unattended", file.name));
            }
        }
        let mut named = names(&file.body)
            .filter(|name| *name == LiveTier::Attended.as_str() || attended.contains(name))
            .collect::<Vec<_>>();
        named.sort_unstable();
        named.dedup();
        for name in named {
            problems.push(if name == LiveTier::Attended.as_str() {
                format!("{}: names the attended tier", file.name)
            } else {
                format!(
                    "{}: names {name}, a provider with attended checks",
                    file.name
                )
            });
        }
    }
    let mut scheduled_tiers = Vec::new();
    for job in jobs
        .iter()
        .filter(|file| file.body.contains("<key>StartCalendarInterval</key>"))
    {
        let interval = if job.body.contains("<key>Weekday</key>") {
            7
        } else {
            1
        };
        let Some(program) = jobs
            .iter()
            .find(|file| job.body.contains(&format!("/{}</string>", file.name)))
        else {
            problems.push(format!("{}: starts no script of {JOBS}", job.name));
            continue;
        };
        let tiers = live_commands(&program.body)
            .iter()
            .flat_map(|command| values(command, "--tier"))
            .collect::<Vec<_>>();
        let [tier] = tiers.as_slice() else {
            problems.push(format!(
                "{}: runs {} with live tiers {tiers:?}, not exactly one",
                job.name, program.name
            ));
            continue;
        };
        let Some(tier) = LiveTier::parse(tier) else {
            problems.push(format!("{}: runs the unknown tier {tier}", job.name));
            continue;
        };
        let declared = tier.schedule_days();
        if declared != Some(interval) {
            problems.push(format!(
                "{}: runs the {} tier every {interval} days, but the tier declares {declared:?}",
                job.name,
                tier.as_str()
            ));
        }
        scheduled_tiers.push(tier);
    }
    for tier in LiveTier::ALL
        .into_iter()
        .filter(|tier| tier.schedule_days().is_some())
    {
        if !scheduled_tiers.contains(&tier) {
            problems.push(format!("no scheduled job runs the {} tier", tier.as_str()));
        }
    }
    for workflow in workflows {
        let recorded = jobs.iter().any(|file| {
            file.body.contains("github_workflows.json")
                && file
                    .body
                    .split(|character: char| {
                        character.is_whitespace() || "()=\"'".contains(character)
                    })
                    .any(|word| word == workflow)
        });
        if !recorded {
            problems.push(format!(
                "the workflow record leaves out the scheduled workflow {workflow}"
            ));
        }
    }
    problems
}

/// The file names of the workflows that run on a schedule.
fn scheduled_workflows(root: &Path) -> Vec<String> {
    let mut workflows = fs::read_dir(root.join(WORKFLOWS))
        .expect("read the workflows")
        .map(|entry| entry.expect("a workflow entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "yml"))
        .filter(|path| {
            let workflow: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&fs::read_to_string(path).expect("read a workflow"))
                    .expect("parse a workflow");
            workflow["on"].get("schedule").is_some()
        })
        .map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("a workflow name")
                .to_owned()
        })
        .collect::<Vec<_>>();
    workflows.sort();
    workflows
}

fn attended_providers() -> Vec<&'static str> {
    let mut providers = declared()
        .into_iter()
        .filter(|entry| entry.check.tier == LiveTier::Attended)
        .filter_map(|entry| entry.provider.map(crate::catalog::ProviderId::name))
        .collect::<Vec<_>>();
    providers.sort_unstable();
    providers.dedup();
    providers
}

#[test]
fn every_scheduled_job_runs_unattended_at_its_tiers_interval_and_records_every_scheduled_workflow()
{
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let problems = schedule_problems(
        &jobs(root),
        &scheduled_workflows(root),
        &attended_providers(),
    );

    assert!(
        problems.is_empty(),
        "a scheduled job runs without a person, so it must never reach the attended tier, and a \
         tier run off its interval reads as overdue or hides a stalled schedule: {problems:#?}"
    );
}

#[test]
fn each_way_a_scheduled_job_breaks_the_schedule_rules_is_reported() {
    let job = |name: &str, body: &str| Job {
        name: name.to_owned(),
        body: body.to_owned(),
    };
    let jobs = [
        job(
            "daily-job",
            "forager doctor --live --tier free\n\
             forager doctor --tier free --live\n\
             \"$bin\" doctor --live --tier=free\n\
             forager doctor \\\n  --live --tier metered\n\
             # forager doctor --live --tier free\n",
        ),
        job(
            "attended-job",
            "forager doctor --live --unattended --tier=attended\n",
        ),
        job(
            "provider-job",
            "# Only for invented_browser.\n\
             forager doctor --unattended --live --provider invented_browser\n",
        ),
        job(
            "weekly-job",
            "forager doctor --live --unattended --tier metered\n\
             workflows=(recorded.yml)\n\
             write \"$state/github_workflows.json\"\n",
        ),
        job(
            "daily.plist",
            "<key>StartCalendarInterval</key>\n\
             <string>/Users/USERNAME/.local/bin/weekly-job</string>\n",
        ),
    ];

    assert_eq!(
        schedule_problems(
            &jobs,
            &["invented.yml".to_owned(), "recorded.yml".to_owned()],
            &["invented_browser"]
        ),
        [
            "daily-job: `forager doctor --live --tier free` lacks --unattended",
            "daily-job: `forager doctor --tier free --live` lacks --unattended",
            "daily-job: `\"$bin\" doctor --live --tier=free` lacks --unattended",
            "daily-job: `forager doctor --live --tier metered` lacks --unattended",
            "attended-job: names the attended tier",
            "provider-job: names invented_browser, a provider with attended checks",
            "daily.plist: runs the metered tier every 1 days, but the tier declares Some(7)",
            "no scheduled job runs the free tier",
            "the workflow record leaves out the scheduled workflow invented.yml",
        ]
    );
}
