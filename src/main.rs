#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use forager::app::{self, Cli};

fn main() -> ExitCode {
    match app::run(Cli::parse()) {
        Ok(output) => {
            if !output.stdout.is_empty() {
                println!("{}", output.stdout);
            }
            if let Some(diagnostic) = output.stderr {
                eprintln!("{diagnostic}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", error.category());
            ExitCode::from(error.exit_code())
        }
    }
}
