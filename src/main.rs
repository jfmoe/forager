#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use forager::app::{self, Cli};

fn main() -> ExitCode {
    match app::run(Cli::parse()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("config_error: {error}");
            ExitCode::from(3)
        }
    }
}
