//! Command-line options.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Command {
    #[default]
    Run,
    Version,
    Help,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub command: Command,
    /// Replay the sample interview with mock providers (no keys needed).
    pub demo: bool,
    /// Run the hardware self-test, print a report and exit.
    pub self_test: bool,
}

pub const HELP: &str = "MeetCue — meeting prompter and interview practice

Usage: MeetCue.exe [options]

  --demo        Replay a sample interview with mock providers (no API keys needed)
  --self-test   Check audio capture, shortcuts, screen-share hiding and providers,
                then print a report
  --version     Print the version
  --help        Show this help
";

pub fn parse(args: &[String]) -> Options {
    let mut o = Options::default();
    for a in args {
        match a.as_str() {
            "--version" | "-V" => o.command = Command::Version,
            "--help" | "-h" | "/?" => o.command = Command::Help,
            "--demo" => o.demo = true,
            "--self-test" => o.self_test = true,
            _ => {}
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let p = |a: &[&str]| parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(p(&[]).command, Command::Run);
        assert_eq!(p(&["--version"]).command, Command::Version);
        assert!(p(&["--demo"]).demo);
        assert!(p(&["--self-test"]).self_test);
        assert_eq!(p(&["--unknown"]), Options::default());
    }
}
