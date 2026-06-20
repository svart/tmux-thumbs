use crate::{colors, state, view};
use clap::{Arg, ArgAction, ArgMatches, Command};
use regex::Regex;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};

#[allow(dead_code)]
fn dbg(msg: &str) {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open("/tmp/thumbs.log")
        .expect("Unable to open log file");

    writeln!(&mut file, "{}", msg).expect("Unable to write log file");
}

type PickerResult<T> = Result<T, PickerError>;

#[derive(Debug)]
enum PickerError {
    Configuration(String),
    Io(io::Error),
}

impl fmt::Display for PickerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PickerError::Configuration(message) => write!(f, "{}", message),
            PickerError::Io(error) => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for PickerError {}

impl From<io::Error> for PickerError {
    fn from(error: io::Error) -> PickerError {
        PickerError::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThumbsColorOptions {
    foreground: String,
    background: String,
    hint_foreground: String,
    hint_background: String,
    select_foreground: String,
    select_background: String,
    multi_foreground: String,
    multi_background: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThumbsOptions {
    alphabet: String,
    format: String,
    colors: ThumbsColorOptions,
    multi: bool,
    reverse: bool,
    unique: bool,
    contrast: bool,
    position: String,
    regexp: Vec<String>,
    target: Option<String>,
}

impl ThumbsOptions {
    fn from_matches(args: &ArgMatches) -> PickerResult<ThumbsOptions> {
        let regexp = args
            .get_many::<String>("regexp")
            .map(|items| items.cloned().collect::<Vec<_>>())
            .unwrap_or_default();

        validate_custom_regexps(&regexp)?;

        Ok(ThumbsOptions {
            alphabet: required_string(args, "alphabet")?.to_string(),
            format: required_string(args, "format")?.to_string(),
            colors: ThumbsColorOptions {
                foreground: required_string(args, "foreground_color")?.to_string(),
                background: required_string(args, "background_color")?.to_string(),
                hint_foreground: required_string(args, "hint_foreground_color")?.to_string(),
                hint_background: required_string(args, "hint_background_color")?.to_string(),
                select_foreground: required_string(args, "select_foreground_color")?.to_string(),
                select_background: required_string(args, "select_background_color")?.to_string(),
                multi_foreground: required_string(args, "multi_foreground_color")?.to_string(),
                multi_background: required_string(args, "multi_background_color")?.to_string(),
            },
            multi: args.get_flag("multi"),
            reverse: args.get_flag("reverse"),
            unique: args.get_flag("unique"),
            contrast: args.get_flag("contrast"),
            position: required_string(args, "position")?.to_string(),
            regexp,
            target: args.get_one::<String>("target").cloned(),
        })
    }
}

fn required_string<'a>(args: &'a ArgMatches, name: &'static str) -> PickerResult<&'a str> {
    args.get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| PickerError::Configuration(format!("missing required option `{}`", name)))
}

fn validate_custom_regexps(regexps: &[String]) -> PickerResult<()> {
    for regexp in regexps {
        Regex::new(regexp).map_err(|error| {
            PickerError::Configuration(format!("Invalid custom regexp `{}`: {}", regexp, error))
        })?;
    }

    Ok(())
}

fn app() -> Command {
    Command::new("thumbs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("A lightning fast version copy/pasting like vimium/vimperator")
        .arg(
            Arg::new("alphabet")
                .help("Sets the alphabet")
                .long("alphabet")
                .short('a')
                .default_value("qwerty"),
        )
        .arg(
            Arg::new("format")
                .help("Specifies the out format for the picked hint. (%U: Upcase, %H: Hint)")
                .long("format")
                .short('f')
                .default_value("%H"),
        )
        .arg(
            Arg::new("foreground_color")
                .help("Sets the foregroud color for matches")
                .long("fg-color")
                .default_value("green"),
        )
        .arg(
            Arg::new("background_color")
                .help("Sets the background color for matches")
                .long("bg-color")
                .default_value("black"),
        )
        .arg(
            Arg::new("hint_foreground_color")
                .help("Sets the foregroud color for hints")
                .long("hint-fg-color")
                .default_value("yellow"),
        )
        .arg(
            Arg::new("hint_background_color")
                .help("Sets the background color for hints")
                .long("hint-bg-color")
                .default_value("black"),
        )
        .arg(
            Arg::new("multi_foreground_color")
                .help("Sets the foreground color for a multi selected item")
                .long("multi-fg-color")
                .default_value("yellow"),
        )
        .arg(
            Arg::new("multi_background_color")
                .help("Sets the background color for a multi selected item")
                .long("multi-bg-color")
                .default_value("black"),
        )
        .arg(
            Arg::new("select_foreground_color")
                .help("Sets the foreground color for selection")
                .long("select-fg-color")
                .default_value("blue"),
        )
        .arg(
            Arg::new("select_background_color")
                .help("Sets the background color for selection")
                .long("select-bg-color")
                .default_value("black"),
        )
        .arg(
            Arg::new("multi")
                .help("Enable multi-selection")
                .long("multi")
                .short('m')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("reverse")
                .help("Reverse the order for assigned hints")
                .long("reverse")
                .short('r')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("unique")
                .help("Don't show duplicated hints for the same match")
                .long("unique")
                .short('u')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("position")
                .help("Hint position")
                .long("position")
                .default_value("left")
                .short('p'),
        )
        .arg(
            Arg::new("regexp")
                .help("Use this regexp as extra pattern to match")
                .long("regexp")
                .short('x')
                .num_args(1)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("contrast")
                .help("Put square brackets around hint for visibility")
                .long("contrast")
                .short('c')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("target")
                .help("Stores the hint in the specified path")
                .long("target")
                .short('t')
                .num_args(1),
        )
}

fn app_args() -> ArgMatches {
    app().get_matches()
}

pub fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => ::std::process::exit(1),
        Err(error) => {
            eprintln!("{}", error);
            ::std::process::exit(1);
        }
    }
}

fn run() -> PickerResult<bool> {
    let args = app_args();
    let options = ThumbsOptions::from_matches(&args)?;

    run_with_options(options)
}

fn run_with_options(options: ThumbsOptions) -> PickerResult<bool> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut output = String::new();

    handle.read_to_string(&mut output)?;

    let lines = output.split('\n').collect::<Vec<&str>>();
    let regexp = options
        .regexp
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    let mut state = state::State::new(&lines, options.alphabet.as_str(), &regexp);

    let selected = {
        let view_options = view::ViewOptions {
            multi: options.multi,
            reverse: options.reverse,
            unique: options.unique,
            contrast: options.contrast,
            position: options.position.as_str(),
        };
        let view_colors = view::ViewColors {
            select_foreground_color: colors::get_color(options.colors.select_foreground.as_str()),
            select_background_color: colors::get_color(options.colors.select_background.as_str()),
            multi_foreground_color: colors::get_color(options.colors.multi_foreground.as_str()),
            multi_background_color: colors::get_color(options.colors.multi_background.as_str()),
            foreground_color: colors::get_color(options.colors.foreground.as_str()),
            background_color: colors::get_color(options.colors.background.as_str()),
            hint_foreground_color: colors::get_color(options.colors.hint_foreground.as_str()),
            hint_background_color: colors::get_color(options.colors.hint_background.as_str()),
        };

        let mut viewbox = view::View::new(&mut state, view_options, view_colors);

        viewbox.present()
    };

    if !selected.is_empty() {
        let output = selected
            .iter()
            .map(|(text, upcase)| {
                let upcase_value = if *upcase { "true" } else { "false" };

                let mut output = options.format.clone();

                output = str::replace(&output, "%U", upcase_value);
                output = str::replace(&output, "%H", text.as_str());
                output
            })
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(target) = options.target.as_deref() {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(target)?;

            file.write_all(output.as_bytes())?;
        } else {
            print!("{}", output);
        }

        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbs_options_parse_clap_defaults() {
        let matches = app().try_get_matches_from(["thumbs"]).unwrap();
        let options = ThumbsOptions::from_matches(&matches).unwrap();

        assert_eq!(options.alphabet, "qwerty");
        assert_eq!(options.format, "%H");
        assert_eq!(options.position, "left");
        assert_eq!(options.colors.foreground, "green");
        assert_eq!(options.colors.background, "black");
        assert_eq!(options.colors.hint_foreground, "yellow");
        assert_eq!(options.colors.hint_background, "black");
        assert_eq!(options.colors.select_foreground, "blue");
        assert_eq!(options.colors.select_background, "black");
        assert_eq!(options.colors.multi_foreground, "yellow");
        assert_eq!(options.colors.multi_background, "black");
        assert!(!options.multi);
        assert!(!options.reverse);
        assert!(!options.unique);
        assert!(!options.contrast);
        assert!(options.regexp.is_empty());
        assert_eq!(options.target, None);
    }

    #[test]
    fn thumbs_options_parse_flags_regexps_and_target() {
        let matches = app()
            .try_get_matches_from([
                "thumbs",
                "--alphabet",
                "numeric",
                "--format",
                "%U:%H",
                "--multi",
                "--reverse",
                "--unique",
                "--contrast",
                "--position",
                "right",
                "--regexp",
                "foo",
                "--regexp",
                "bar",
                "--target",
                "/tmp/thumbs-output",
            ])
            .unwrap();
        let options = ThumbsOptions::from_matches(&matches).unwrap();

        assert_eq!(options.alphabet, "numeric");
        assert_eq!(options.format, "%U:%H");
        assert_eq!(options.position, "right");
        assert!(options.multi);
        assert!(options.reverse);
        assert!(options.unique);
        assert!(options.contrast);
        assert_eq!(options.regexp, ["foo", "bar"]);
        assert_eq!(options.target.as_deref(), Some("/tmp/thumbs-output"));
    }

    #[test]
    fn thumbs_options_reject_invalid_regexp() {
        let matches = app()
            .try_get_matches_from(["thumbs", "--regexp", "["])
            .unwrap();
        let error = ThumbsOptions::from_matches(&matches).unwrap_err();

        assert!(error.to_string().contains("Invalid custom regexp"));
    }
}
