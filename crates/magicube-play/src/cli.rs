use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Options {
    pub level: Option<PathBuf>,
    pub solutions_dir: Option<PathBuf>,
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
}
