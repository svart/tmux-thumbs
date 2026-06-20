use crate::{alphabets, colors, view};
use regex::Regex;

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn tmux_option_value(raw: &str) -> &str {
    let value = raw.trim();

    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerArg {
    Flag(String),
    Value { name: String, value: String },
    Regexp(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PickerArgs {
    args: Vec<PickerArg>,
}

impl PickerArgs {
    pub(crate) fn from_tmux_options(options: &str) -> Result<PickerArgs, String> {
        let pattern = Regex::new(r#"^@thumbs-([\w\-0-9]+)\s+(.+)$"#).unwrap();
        let mut args = Vec::new();

        for line in options.lines() {
            let Some(captures) = pattern.captures(line) else {
                continue;
            };

            let name = captures.get(1).unwrap().as_str();
            let value = tmux_option_value(captures.get(2).unwrap().as_str());

            if is_boolean_picker_arg(name) {
                if is_true_tmux_value(value) {
                    args.push(PickerArg::Flag(name.to_string()));
                }

                continue;
            }

            if value.is_empty() {
                continue;
            }

            if is_string_picker_arg(name) {
                let value = validate_picker_value(name, value)?;
                args.push(PickerArg::Value {
                    name: name.to_string(),
                    value,
                });
            } else if name.starts_with("regexp") {
                let regexp = value.replace("\\\\", "\\");
                Regex::new(&regexp)
                    .map_err(|error| format!("Invalid custom regexp `{}`: {}", regexp, error))?;
                args.push(PickerArg::Regexp(regexp));
            }
        }

        Ok(PickerArgs { args })
    }

    pub(crate) fn render_shell_args(&self) -> Vec<String> {
        self.args
            .iter()
            .flat_map(|arg| match arg {
                PickerArg::Flag(name) => vec![format!("--{}", name)],
                PickerArg::Value { name, value } => vec![format!("--{}", name), shell_quote(value)],
                PickerArg::Regexp(value) => vec!["--regexp".to_string(), shell_quote(value)],
            })
            .collect()
    }
}

fn is_boolean_picker_arg(name: &str) -> bool {
    ["reverse", "unique", "contrast"].contains(&name)
}

fn is_string_picker_arg(name: &str) -> bool {
    ["alphabet", "position"].contains(&name) || is_color_picker_arg(name)
}

fn is_color_picker_arg(name: &str) -> bool {
    [
        "fg-color",
        "bg-color",
        "hint-bg-color",
        "hint-fg-color",
        "select-fg-color",
        "select-bg-color",
        "multi-fg-color",
        "multi-bg-color",
    ]
    .contains(&name)
}

fn validate_picker_value(name: &str, value: &str) -> Result<String, String> {
    match name {
        "alphabet" => {
            alphabets::validate_alphabet(value).map_err(|error| error.to_string())?;
            Ok(value.to_string())
        }
        "position" => view::HintPosition::parse(value)
            .map(|position| position.as_str().to_string())
            .map_err(|error| error.to_string()),
        name if is_color_picker_arg(name) => {
            colors::ColorSpec::parse(value).map_err(|error| error.to_string())?;
            Ok(value.to_string())
        }
        _ => Ok(value.to_string()),
    }
}

fn is_true_tmux_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "enabled" | "true" | "yes" | "on"
    )
}
