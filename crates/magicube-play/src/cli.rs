use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use magicube_solver::GameSettings;

#[derive(Debug, Default)]
pub struct Options {
    pub level: Option<PathBuf>,
    pub solutions_dir: Option<PathBuf>,
    pub replay: Option<PathBuf>,
    pub solve: bool,
    pub settings: GameSettings,
    pub help: bool,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> io::Result<Self> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        let mut positional_only = false;
        while let Some(arg) = args.next() {
            if !positional_only {
                if arg == "--help" || arg == "-h" {
                    options.help = true;
                    return Ok(options);
                }
                if arg == "--" {
                    positional_only = true;
                    continue;
                }
                if arg == "--solve" {
                    options.solve = true;
                    continue;
                }
                if arg == "--airborne-shooting" {
                    options.settings.allow_airborne_shooting = true;
                    continue;
                }
                if arg == "--airborne-pushing" {
                    options.settings.allow_airborne_pushing = true;
                    continue;
                }
                if arg == "--replay" {
                    let path = args
                        .next()
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| invalid("--replay requires a solution JSON path"))?;
                    if options.replay.replace(PathBuf::from(path)).is_some() {
                        return Err(invalid("expected only one --replay file"));
                    }
                    continue;
                }
                if arg == "--solutions-dir" {
                    let directory = args
                        .next()
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| invalid("--solutions-dir requires a directory path"))?;
                    options.solutions_dir = Some(PathBuf::from(directory));
                    continue;
                }
                if arg.to_string_lossy().starts_with('-') {
                    return Err(invalid(format!(
                        "unknown option {} (use --help for usage)",
                        arg.display()
                    )));
                }
            }
            if options.level.replace(PathBuf::from(arg)).is_some() {
                return Err(invalid("expected at most one level file"));
            }
        }
        if options.replay.is_some() && (options.solve || options.level.is_some()) {
            return Err(invalid(
                "--replay cannot be combined with --solve or a level file; the replay contains its own map",
            ));
        }
        if options.solutions_dir.is_some() && (options.replay.is_some() || options.solve) {
            return Err(invalid(
                "--solutions-dir is only available when playing; replay does not save attempts",
            ));
        }
        if options.settings != GameSettings::default()
            && (options.replay.is_some() || options.solve)
        {
            return Err(invalid(
                "--airborne-shooting and --airborne-pushing apply to manual play; solver launches use default rules and saved replays use their recorded rules",
            ));
        }
        Ok(options)
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airborne_rules_are_independent_opt_ins_and_cannot_override_replay_or_solver_rules() {
        assert_eq!(
            Options::parse([]).unwrap().settings,
            GameSettings::default()
        );
        let options =
            Options::parse(["--airborne-shooting", "level.txt"].map(OsString::from)).unwrap();
        assert!(options.settings.allow_airborne_shooting);
        assert!(!options.settings.allow_airborne_pushing);
        let pushing =
            Options::parse(["--airborne-pushing", "level.txt"].map(OsString::from)).unwrap();
        assert!(pushing.settings.allow_airborne_pushing);
        assert!(!pushing.settings.allow_airborne_shooting);
        let both =
            Options::parse(["--airborne-shooting", "--airborne-pushing"].map(OsString::from))
                .unwrap();
        assert!(both.settings.allow_airborne_shooting && both.settings.allow_airborne_pushing);
        for args in [
            vec!["--airborne-shooting", "--solve"],
            vec!["--airborne-shooting", "--replay", "saved.json"],
            vec!["--airborne-pushing", "--solve"],
            vec!["--airborne-pushing", "--replay", "saved.json"],
        ] {
            assert!(Options::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[test]
    fn accepts_a_directory_override_before_or_after_the_level() {
        for args in [
            vec!["--solutions-dir", "my saves", "level.txt"],
            vec!["level.txt", "--solutions-dir", "my saves"],
        ] {
            let options = Options::parse(args.into_iter().map(OsString::from)).unwrap();
            assert_eq!(options.level, Some(PathBuf::from("level.txt")));
            assert_eq!(options.solutions_dir, Some(PathBuf::from("my saves")));
        }
        for args in [
            vec!["--solutions-dir"],
            vec!["one.txt", "two.txt"],
            vec!["--unknown"],
        ] {
            assert!(Options::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[test]
    fn accepts_saved_replays_and_solver_replays_with_an_optional_level() {
        let parse = |args: Vec<&str>| Options::parse(args.into_iter().map(OsString::from)).unwrap();
        let saved = parse(vec!["--replay", "my solution.json"]);
        assert_eq!(saved.replay, Some(PathBuf::from("my solution.json")));
        assert!(saved.level.is_none());
        assert!(!saved.solve);
        for args in [vec!["--solve", "level.txt"], vec!["level.txt", "--solve"]] {
            let solver = parse(args);
            assert!(solver.solve);
            assert_eq!(solver.level, Some(PathBuf::from("level.txt")));
        }
        let selector = parse(vec!["--solve"]);
        assert!(selector.solve);
        assert!(selector.level.is_none());
        assert_eq!(
            parse(vec!["--", "--solve"]).level,
            Some(PathBuf::from("--solve"))
        );
    }

    #[test]
    fn rejects_missing_replay_paths_and_conflicting_modes() {
        for args in [
            vec!["--replay"],
            vec!["--replay", ""],
            vec!["--replay", "one.json", "--replay", "two.json"],
            vec!["--replay", "saved.json", "level.txt"],
            vec!["--solve", "--replay", "saved.json"],
            vec!["--replay", "saved.json", "--solutions-dir", "saves"],
            vec!["--solve", "--solutions-dir", "saves"],
        ] {
            assert!(Options::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
