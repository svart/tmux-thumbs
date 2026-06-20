use super::*;
use std::char;
use std::fmt;
use std::io::{stdout, Read, Write};
use termion::async_stdin;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::{color, cursor};

use unicode_width::UnicodeWidthStr;

pub struct View<'a> {
    state: &'a mut state::State<'a>,
    skip: usize,
    multi: bool,
    contrast: bool,
    position: HintPosition,
    matches: Vec<state::Match<'a>>,
    select_foreground_color: Box<dyn color::Color>,
    select_background_color: Box<dyn color::Color>,
    multi_foreground_color: Box<dyn color::Color>,
    multi_background_color: Box<dyn color::Color>,
    foreground_color: Box<dyn color::Color>,
    background_color: Box<dyn color::Color>,
    hint_background_color: Box<dyn color::Color>,
    hint_foreground_color: Box<dyn color::Color>,
    chosen: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintPosition {
    Left,
    Right,
    OffLeft,
    OffRight,
}

impl HintPosition {
    pub fn parse(value: &str) -> Result<HintPosition, HintPositionParseError> {
        match value {
            "left" => Ok(HintPosition::Left),
            "right" => Ok(HintPosition::Right),
            "off_left" => Ok(HintPosition::OffLeft),
            "off_right" => Ok(HintPosition::OffRight),
            _ => Err(HintPositionParseError {
                value: value.to_string(),
            }),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HintPosition::Left => "left",
            HintPosition::Right => "right",
            HintPosition::OffLeft => "off_left",
            HintPosition::OffRight => "off_right",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintPositionParseError {
    value: String,
}

impl fmt::Display for HintPositionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown hint position: {}", self.value)
    }
}

impl std::error::Error for HintPositionParseError {}

enum CaptureEvent {
    Exit,
    Hint,
}

pub struct ViewOptions {
    pub multi: bool,
    pub reverse: bool,
    pub unique: bool,
    pub contrast: bool,
    pub position: HintPosition,
}

pub struct ViewColors {
    pub select_foreground_color: Box<dyn color::Color>,
    pub select_background_color: Box<dyn color::Color>,
    pub multi_foreground_color: Box<dyn color::Color>,
    pub multi_background_color: Box<dyn color::Color>,
    pub foreground_color: Box<dyn color::Color>,
    pub background_color: Box<dyn color::Color>,
    pub hint_foreground_color: Box<dyn color::Color>,
    pub hint_background_color: Box<dyn color::Color>,
}

impl<'a> View<'a> {
    pub fn new(
        state: &'a mut state::State<'a>,
        options: ViewOptions,
        colors: ViewColors,
    ) -> View<'a> {
        let matches = state.matches(options.reverse, options.unique);
        let skip = if options.reverse {
            matches.len().saturating_sub(1)
        } else {
            0
        };

        View {
            state,
            skip,
            multi: options.multi,
            contrast: options.contrast,
            position: options.position,
            matches,
            select_foreground_color: colors.select_foreground_color,
            select_background_color: colors.select_background_color,
            multi_foreground_color: colors.multi_foreground_color,
            multi_background_color: colors.multi_background_color,
            foreground_color: colors.foreground_color,
            background_color: colors.background_color,
            hint_foreground_color: colors.hint_foreground_color,
            hint_background_color: colors.hint_background_color,
            chosen: vec![],
        }
    }

    pub fn prev(&mut self) {
        if self.skip > 0 {
            self.skip -= 1;
        }
    }

    pub fn next(&mut self) {
        if self.skip < self.matches.len().saturating_sub(1) {
            self.skip += 1;
        }
    }

    fn make_hint_text(&self, hint: &str) -> String {
        if self.contrast {
            format!("[{}]", hint)
        } else {
            hint.to_string()
        }
    }

    fn render(&self, stdout: &mut dyn Write, typed_hint: &str) {
        write!(stdout, "{}", cursor::Hide).unwrap();

        for (index, line) in self.state.lines.iter().enumerate() {
            let clean = line.trim_end_matches(|c: char| c.is_whitespace());

            if !clean.is_empty() {
                print!(
                    "{goto}{text}",
                    goto = cursor::Goto(1, terminal_position(index)),
                    text = line
                );
            }
        }

        let selected = self.matches.get(self.skip);

        for mat in self.matches.iter() {
            let chosen_hint = self.chosen.iter().any(|(hint, _)| hint == mat.text);

            let selected_color = if chosen_hint {
                &self.multi_foreground_color
            } else if selected == Some(mat) {
                &self.select_foreground_color
            } else {
                &self.foreground_color
            };
            let selected_background_color = if chosen_hint {
                &self.multi_background_color
            } else if selected == Some(mat) {
                &self.select_background_color
            } else {
                &self.background_color
            };

            // Match coordinates are byte indexes; rendering needs display columns.
            let line = &self.state.lines[mat.y];
            let match_column = display_column(line, mat.x);
            let match_row = terminal_position(mat.y);
            let text = self.make_hint_text(mat.text);

            print!(
                "{goto}{background}{foregroud}{text}{resetf}{resetb}",
                goto = cursor::Goto(terminal_position(match_column), match_row),
                foregroud = color::Fg(&**selected_color),
                background = color::Bg(&**selected_background_color),
                resetf = color::Fg(color::Reset),
                resetb = color::Bg(color::Reset),
                text = &text
            );

            if let Some(ref hint) = mat.hint {
                let extra_position =
                    hint_offset(self.position, text.width_cjk(), hint.len(), self.contrast);

                let text = self.make_hint_text(hint.as_str());
                let final_position = hint_terminal_position(match_column, extra_position);

                print!(
                    "{goto}{background}{foregroud}{text}{resetf}{resetb}",
                    goto = cursor::Goto(final_position, match_row),
                    foregroud = color::Fg(&*self.hint_foreground_color),
                    background = color::Bg(&*self.hint_background_color),
                    resetf = color::Fg(color::Reset),
                    resetb = color::Bg(color::Reset),
                    text = &text
                );

                if hint.starts_with(typed_hint) {
                    print!(
                        "{goto}{background}{foregroud}{text}{resetf}{resetb}",
                        goto = cursor::Goto(final_position, match_row),
                        foregroud = color::Fg(&*self.multi_foreground_color),
                        background = color::Bg(&*self.multi_background_color),
                        resetf = color::Fg(color::Reset),
                        resetb = color::Bg(color::Reset),
                        text = &typed_hint
                    );
                }
            }
        }

        stdout.flush().unwrap();
    }

    fn listen(&mut self, stdin: &mut dyn Read, stdout: &mut dyn Write) -> CaptureEvent {
        if self.matches.is_empty() {
            return CaptureEvent::Exit;
        }

        let mut typed_hint: String = "".to_owned();
        let longest_hint = self
            .matches
            .iter()
            .filter_map(|m| m.hint.clone())
            .max_by(|x, y| x.len().cmp(&y.len()))
            .unwrap()
            .clone();

        self.render(stdout, &typed_hint);

        loop {
            match stdin.keys().next() {
                Some(key) => {
                    match key {
                        Ok(key) => {
                            match key {
                                Key::Esc => {
                                    if self.multi && !typed_hint.is_empty() {
                                        typed_hint.clear();
                                    } else {
                                        break;
                                    }
                                }
                                Key::Up => {
                                    self.prev();
                                }
                                Key::Down => {
                                    self.next();
                                }
                                Key::Left => {
                                    self.prev();
                                }
                                Key::Right => {
                                    self.next();
                                }
                                Key::Backspace => {
                                    typed_hint.pop();
                                }
                                Key::Char(ch) => {
                                    match ch {
                                        '\n' => match self
                                            .matches
                                            .iter()
                                            .enumerate()
                                            .find(|&h| h.0 == self.skip)
                                        {
                                            Some(hm) => {
                                                self.chosen.push((hm.1.text.to_string(), false));

                                                if !self.multi {
                                                    return CaptureEvent::Hint;
                                                }
                                            }
                                            _ => panic!("Match not found?"),
                                        },
                                        ' ' => {
                                            if self.multi {
                                                // Finalize the multi selection
                                                return CaptureEvent::Hint;
                                            } else {
                                                // Enable the multi selection
                                                self.multi = true;
                                            }
                                        }
                                        key => {
                                            let key = key.to_string();
                                            let lower_key = key.to_lowercase();

                                            typed_hint.push_str(lower_key.as_str());

                                            let selection = self
                                                .matches
                                                .iter()
                                                .find(|mat| mat.hint == Some(typed_hint.clone()));

                                            match selection {
                                                Some(mat) => {
                                                    self.chosen.push((
                                                        mat.text.to_string(),
                                                        key != lower_key,
                                                    ));

                                                    if self.multi {
                                                        typed_hint.clear();
                                                    } else {
                                                        return CaptureEvent::Hint;
                                                    }
                                                }
                                                None => {
                                                    if !self.multi
                                                        && typed_hint.len() >= longest_hint.len()
                                                    {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    // Unknown key
                                }
                            }
                        }
                        Err(err) => panic!("{}", err),
                    }

                    stdin
                        .keys()
                        .for_each(|_| { /* Skip the rest of stdin buffer */ })
                }
                _ => {
                    // Nothing in the buffer. Wait for a bit...
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue; // don't render again if nothing new to show
                }
            }

            self.render(stdout, &typed_hint);
        }

        CaptureEvent::Exit
    }

    pub fn present(&mut self) -> Vec<(String, bool)> {
        let mut stdin = async_stdin();
        let mut stdout = stdout()
            .into_raw_mode()
            .unwrap()
            .into_alternate_screen()
            .unwrap();

        let hints = match self.listen(&mut stdin, &mut stdout) {
            CaptureEvent::Exit => vec![],
            CaptureEvent::Hint => self.chosen.clone(),
        };

        write!(stdout, "{}", cursor::Show).unwrap();

        hints
    }
}

fn terminal_position(zero_based: usize) -> u16 {
    zero_based.saturating_add(1).min(u16::MAX as usize) as u16
}

fn display_column(line: &str, byte_index: usize) -> usize {
    line.get(..byte_index)
        .expect("match coordinate must be on a character boundary")
        .width_cjk()
}

fn hint_offset(
    position: HintPosition,
    match_text_width: usize,
    hint_width: usize,
    contrast: bool,
) -> isize {
    match position {
        HintPosition::Left => 0,
        HintPosition::Right => match_text_width as isize - hint_width as isize,
        HintPosition::OffLeft => -(hint_width as isize) - if contrast { 2 } else { 0 },
        HintPosition::OffRight => match_text_width as isize,
    }
}

fn hint_terminal_position(match_column: usize, offset: isize) -> u16 {
    let column = match_column as isize + offset;

    if column <= 0 {
        return 1;
    }

    terminal_position(column as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(output: &str) -> Vec<&str> {
        output.split('\n').collect::<Vec<&str>>()
    }

    fn default_colors() -> ViewColors {
        ViewColors {
            select_foreground_color: colors::get_color("default"),
            select_background_color: colors::get_color("default"),
            multi_foreground_color: colors::get_color("default"),
            multi_background_color: colors::get_color("default"),
            foreground_color: colors::get_color("default"),
            background_color: colors::get_color("default"),
            hint_background_color: colors::get_color("default"),
            hint_foreground_color: colors::get_color("default"),
        }
    }

    #[test]
    fn hint_text() {
        let lines = split("lorem 127.0.0.1 lorem");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let mut view = View {
            state: &mut state,
            skip: 0,
            multi: false,
            contrast: false,
            position: HintPosition::Left,
            matches: vec![],
            select_foreground_color: colors::get_color("default"),
            select_background_color: colors::get_color("default"),
            multi_foreground_color: colors::get_color("default"),
            multi_background_color: colors::get_color("default"),
            foreground_color: colors::get_color("default"),
            background_color: colors::get_color("default"),
            hint_background_color: colors::get_color("default"),
            hint_foreground_color: colors::get_color("default"),
            chosen: vec![],
        };

        let result = view.make_hint_text("a");
        assert_eq!(result, "a".to_string());

        view.contrast = true;
        let result = view.make_hint_text("a");
        assert_eq!(result, "[a]".to_string());
    }

    #[test]
    fn hint_position_parse_valid_values() {
        assert_eq!(HintPosition::parse("left").unwrap(), HintPosition::Left);
        assert_eq!(HintPosition::parse("right").unwrap(), HintPosition::Right);
        assert_eq!(
            HintPosition::parse("off_left").unwrap(),
            HintPosition::OffLeft
        );
        assert_eq!(
            HintPosition::parse("off_right").unwrap(),
            HintPosition::OffRight
        );
        assert_eq!(HintPosition::OffRight.as_str(), "off_right");
    }

    #[test]
    fn hint_position_rejects_invalid_value() {
        let error = HintPosition::parse("center").unwrap_err();

        assert_eq!(error.to_string(), "Unknown hint position: center");
    }

    #[test]
    fn reverse_empty_matches_does_not_panic() {
        let lines = split("no hints here");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            ViewOptions {
                multi: false,
                reverse: true,
                unique: false,
                contrast: false,
                position: HintPosition::Left,
            },
            default_colors(),
        );

        assert_eq!(view.skip, 0);
        assert!(view.matches.is_empty());
    }

    #[test]
    fn off_left_hint_position_saturates_at_first_terminal_column() {
        let offset = hint_offset(HintPosition::OffLeft, 1, 2, false);
        assert_eq!(hint_terminal_position(0, offset), 1);

        let contrast_offset = hint_offset(HintPosition::OffLeft, 1, 2, true);
        assert_eq!(hint_terminal_position(0, contrast_offset), 1);
    }

    #[test]
    fn display_column_uses_unicode_width() {
        let line = "你λ 127.0.0.1";

        assert_eq!(display_column(line, "你λ ".len()), 4);
    }
}
