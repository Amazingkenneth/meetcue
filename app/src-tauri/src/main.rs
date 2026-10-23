// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use meetcue_app::cli::{self, Command};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = cli::parse(&args);
    match opts.command {
        Command::Version => {
            meetcue_app::console::attach();
            println!("MeetCue {}", meetcue_core::VERSION);
        }
        Command::Help => {
            meetcue_app::console::attach();
            print!("{}", cli::HELP);
        }
        Command::Run => {
            if opts.self_test {
                meetcue_app::console::attach();
            }
            std::process::exit(meetcue_app::run(opts));
        }
    }
}
