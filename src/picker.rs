use crate::{colors, state, view};
use clap::{Arg, ArgAction, Command};
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

fn app_args() -> clap::ArgMatches {
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
        .get_matches()
}

pub fn main() {
    let args = app_args();
    let format = args.get_one::<String>("format").unwrap().as_str();
    let alphabet = args.get_one::<String>("alphabet").unwrap().as_str();
    let position = args.get_one::<String>("position").unwrap().as_str();
    let target = args.get_one::<String>("target").map(String::as_str);
    let multi = args.get_flag("multi");
    let reverse = args.get_flag("reverse");
    let unique = args.get_flag("unique");
    let contrast = args.get_flag("contrast");
    let regexp = args
        .get_many::<String>("regexp")
        .map(|items| items.map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let foreground_color =
        colors::get_color(args.get_one::<String>("foreground_color").unwrap().as_str());
    let background_color =
        colors::get_color(args.get_one::<String>("background_color").unwrap().as_str());
    let hint_foreground_color = colors::get_color(
        args.get_one::<String>("hint_foreground_color")
            .unwrap()
            .as_str(),
    );
    let hint_background_color = colors::get_color(
        args.get_one::<String>("hint_background_color")
            .unwrap()
            .as_str(),
    );
    let select_foreground_color = colors::get_color(
        args.get_one::<String>("select_foreground_color")
            .unwrap()
            .as_str(),
    );
    let select_background_color = colors::get_color(
        args.get_one::<String>("select_background_color")
            .unwrap()
            .as_str(),
    );
    let multi_foreground_color = colors::get_color(
        args.get_one::<String>("multi_foreground_color")
            .unwrap()
            .as_str(),
    );
    let multi_background_color = colors::get_color(
        args.get_one::<String>("multi_background_color")
            .unwrap()
            .as_str(),
    );

    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut output = String::new();

    handle.read_to_string(&mut output).unwrap();

    let lines = output.split('\n').collect::<Vec<&str>>();

    let mut state = state::State::new(&lines, alphabet, &regexp);

    let selected = {
        let options = view::ViewOptions {
            multi,
            reverse,
            unique,
            contrast,
            position,
        };
        let colors = view::ViewColors {
            select_foreground_color,
            select_background_color,
            multi_foreground_color,
            multi_background_color,
            foreground_color,
            background_color,
            hint_foreground_color,
            hint_background_color,
        };

        let mut viewbox = view::View::new(&mut state, options, colors);

        viewbox.present()
    };

    if !selected.is_empty() {
        let output = selected
            .iter()
            .map(|(text, upcase)| {
                let upcase_value = if *upcase { "true" } else { "false" };

                let mut output = format.to_string();

                output = str::replace(&output, "%U", upcase_value);
                output = str::replace(&output, "%H", text.as_str());
                output
            })
            .collect::<Vec<_>>()
            .join("\n");

        if let Some(target) = target {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(target)
                .expect("Unable to open the target file");

            file.write_all(output.as_bytes()).unwrap();
        } else {
            print!("{}", output);
        }
    } else {
        ::std::process::exit(1);
    }
}
