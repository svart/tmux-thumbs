use super::*;
use std::char;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureEvent {
    Exit,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyOutcome {
    Continue,
    Exit,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputMatch<'a> {
    text: &'a str,
    hint: Option<&'a str>,
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
        self.skip = previous_index(self.skip);
    }

    pub fn next(&mut self) {
        self.skip = next_index(self.skip, self.matches.len());
    }

    fn make_hint_text(&self, hint: &str) -> String {
        if self.contrast {
            format!("[{}]", hint)
        } else {
            hint.to_string()
        }
    }

    fn render(&self, stdout: &mut dyn Write, typed_hint: &str) -> io::Result<()> {
        write!(stdout, "{}", cursor::Hide)?;

        for (index, line) in self.state.lines().iter().enumerate() {
            let clean = line.trim_end_matches(|c: char| c.is_whitespace());

            if !clean.is_empty() {
                write!(
                    stdout,
                    "{goto}{text}",
                    goto = cursor::Goto(1, terminal_position(index)),
                    text = line
                )?;
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
            let line = &self.state.lines()[mat.y];
            let match_column = display_column(line, mat.x);
            let match_row = terminal_position(mat.y);
            let text = self.make_hint_text(mat.text);

            write!(
                stdout,
                "{goto}{background}{foreground}{text}{resetf}{resetb}",
                goto = cursor::Goto(terminal_position(match_column), match_row),
                foreground = color::Fg(&**selected_color),
                background = color::Bg(&**selected_background_color),
                resetf = color::Fg(color::Reset),
                resetb = color::Bg(color::Reset),
                text = &text
            )?;

            if let Some(ref hint) = mat.hint {
                let match_text_width = text.width_cjk();
                let hint_text = self.make_hint_text(hint.as_str());
                let final_position = hint_column(
                    self.position,
                    match_column,
                    match_text_width,
                    hint.len(),
                    self.contrast,
                );

                write!(
                    stdout,
                    "{goto}{background}{foreground}{text}{resetf}{resetb}",
                    goto = cursor::Goto(final_position, match_row),
                    foreground = color::Fg(&*self.hint_foreground_color),
                    background = color::Bg(&*self.hint_background_color),
                    resetf = color::Fg(color::Reset),
                    resetb = color::Bg(color::Reset),
                    text = &hint_text
                )?;

                if hint.starts_with(typed_hint) {
                    write!(
                        stdout,
                        "{goto}{background}{foreground}{text}{resetf}{resetb}",
                        goto = cursor::Goto(final_position, match_row),
                        foreground = color::Fg(&*self.multi_foreground_color),
                        background = color::Bg(&*self.multi_background_color),
                        resetf = color::Fg(color::Reset),
                        resetb = color::Bg(color::Reset),
                        text = &typed_hint
                    )?;
                }
            }
        }

        stdout.flush()
    }

    fn listen(&mut self, stdin: &mut dyn Read, stdout: &mut dyn Write) -> io::Result<CaptureEvent> {
        if self.matches.is_empty() {
            return Ok(CaptureEvent::Exit);
        }

        let mut typed_hint: String = "".to_owned();
        let input_matches = self
            .matches
            .iter()
            .map(|mat| InputMatch {
                text: mat.text,
                hint: mat.hint.as_deref(),
            })
            .collect::<Vec<_>>();
        let longest_hint_len = input_matches
            .iter()
            .filter_map(|mat| mat.hint.map(str::len))
            .max()
            .expect("matches must have hints");

        self.render(stdout, &typed_hint)?;

        loop {
            match stdin.keys().next() {
                Some(key) => {
                    match key {
                        Ok(key) => {
                            match handle_key(
                                key,
                                &mut self.multi,
                                &mut self.skip,
                                &mut typed_hint,
                                &mut self.chosen,
                                &input_matches,
                                longest_hint_len,
                            ) {
                                KeyOutcome::Continue => {}
                                KeyOutcome::Exit => break,
                                KeyOutcome::Hint => return Ok(CaptureEvent::Hint),
                            }
                        }
                        Err(err) => return Err(err),
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

            self.render(stdout, &typed_hint)?;
        }

        Ok(CaptureEvent::Exit)
    }

    pub fn present(&mut self) -> io::Result<Vec<(String, bool)>> {
        let mut stdin = async_stdin();
        let terminal = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let mut stdout = terminal.into_raw_mode()?.into_alternate_screen()?;

        let hints = match self.listen(&mut stdin, &mut stdout)? {
            CaptureEvent::Exit => vec![],
            CaptureEvent::Hint => self.chosen.clone(),
        };

        write!(stdout, "{}", cursor::Show)?;

        Ok(hints)
    }
}

fn previous_index(index: usize) -> usize {
    index.saturating_sub(1)
}

fn next_index(index: usize, len: usize) -> usize {
    if index < len.saturating_sub(1) {
        index + 1
    } else {
        index
    }
}

fn handle_key(
    key: Key,
    multi: &mut bool,
    skip: &mut usize,
    typed_hint: &mut String,
    chosen: &mut Vec<(String, bool)>,
    matches: &[InputMatch<'_>],
    longest_hint_len: usize,
) -> KeyOutcome {
    match key {
        Key::Esc => {
            if *multi && !typed_hint.is_empty() {
                typed_hint.clear();
                KeyOutcome::Continue
            } else {
                KeyOutcome::Exit
            }
        }
        Key::Up | Key::Left => {
            *skip = previous_index(*skip);
            KeyOutcome::Continue
        }
        Key::Down | Key::Right => {
            *skip = next_index(*skip, matches.len());
            KeyOutcome::Continue
        }
        Key::Backspace => {
            typed_hint.pop();
            KeyOutcome::Continue
        }
        Key::Char('\n') => {
            let mat = matches.get(*skip).expect("Match not found?");
            chosen.push((mat.text.to_string(), false));

            if *multi {
                KeyOutcome::Continue
            } else {
                KeyOutcome::Hint
            }
        }
        Key::Char(' ') => {
            if *multi {
                KeyOutcome::Hint
            } else {
                *multi = true;
                KeyOutcome::Continue
            }
        }
        Key::Char(ch) => {
            handle_hint_char(ch, *multi, typed_hint, chosen, matches, longest_hint_len)
        }
        _ => KeyOutcome::Continue,
    }
}

fn handle_hint_char(
    ch: char,
    multi: bool,
    typed_hint: &mut String,
    chosen: &mut Vec<(String, bool)>,
    matches: &[InputMatch<'_>],
    longest_hint_len: usize,
) -> KeyOutcome {
    let key = ch.to_string();
    let lower_key = key.to_lowercase();

    typed_hint.push_str(lower_key.as_str());

    let selection = matches
        .iter()
        .find(|mat| mat.hint == Some(typed_hint.as_str()));

    if let Some(mat) = selection {
        chosen.push((mat.text.to_string(), key != lower_key));

        if multi {
            typed_hint.clear();
            KeyOutcome::Continue
        } else {
            KeyOutcome::Hint
        }
    } else if !multi && typed_hint.len() >= longest_hint_len {
        KeyOutcome::Exit
    } else {
        KeyOutcome::Continue
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

fn hint_column(
    position: HintPosition,
    match_column: usize,
    match_text_width: usize,
    hint_width: usize,
    contrast: bool,
) -> u16 {
    let offset = hint_offset(position, match_text_width, hint_width, contrast);

    hint_terminal_position(match_column, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(output: &str) -> Box<[&str]> {
        output.split('\n').collect::<Vec<&str>>().into_boxed_slice()
    }

    fn default_colors() -> ViewColors {
        ViewColors {
            select_foreground_color: test_color("default"),
            select_background_color: test_color("default"),
            multi_foreground_color: test_color("default"),
            multi_background_color: test_color("default"),
            foreground_color: test_color("default"),
            background_color: test_color("default"),
            hint_background_color: test_color("default"),
            hint_foreground_color: test_color("default"),
        }
    }

    fn test_color(name: &str) -> Box<dyn color::Color> {
        colors::get_color(name).unwrap()
    }

    fn default_options(position: HintPosition, contrast: bool) -> ViewOptions {
        ViewOptions {
            multi: false,
            reverse: false,
            unique: false,
            contrast,
            position,
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("write failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FlushFailingWriter;

    impl Write for FlushFailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failed"))
        }
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }

    fn input_matches() -> [InputMatch<'static>; 2] {
        [
            InputMatch {
                text: "alpha",
                hint: Some("a"),
            },
            InputMatch {
                text: "bravo",
                hint: Some("b"),
            },
        ]
    }

    #[test]
    fn lowercase_hint_selects_match() {
        let matches = input_matches();
        let mut multi = false;
        let mut skip = 0;
        let mut typed_hint = String::new();
        let mut chosen = vec![];

        let outcome = handle_key(
            Key::Char('a'),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(outcome, KeyOutcome::Hint);
        assert_eq!(chosen, [("alpha".to_string(), false)]);
    }

    #[test]
    fn uppercase_hint_flags_selection_for_upcase_command() {
        let matches = input_matches();
        let mut multi = false;
        let mut skip = 0;
        let mut typed_hint = String::new();
        let mut chosen = vec![];

        let outcome = handle_key(
            Key::Char('A'),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(outcome, KeyOutcome::Hint);
        assert_eq!(chosen, [("alpha".to_string(), true)]);
    }

    #[test]
    fn multi_selection_collects_hints_and_space_finalizes() {
        let matches = input_matches();
        let mut multi = true;
        let mut skip = 0;
        let mut typed_hint = String::new();
        let mut chosen = vec![];

        let selection_outcome = handle_key(
            Key::Char('a'),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(selection_outcome, KeyOutcome::Continue);
        assert_eq!(typed_hint, "");
        assert_eq!(chosen, [("alpha".to_string(), false)]);

        let finalize_outcome = handle_key(
            Key::Char(' '),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(finalize_outcome, KeyOutcome::Hint);
    }

    #[test]
    fn escape_and_backspace_update_typed_hint() {
        let matches = input_matches();
        let mut multi = true;
        let mut skip = 0;
        let mut typed_hint = "ab".to_string();
        let mut chosen = vec![];

        let backspace_outcome = handle_key(
            Key::Backspace,
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(backspace_outcome, KeyOutcome::Continue);
        assert_eq!(typed_hint, "a");

        let escape_outcome = handle_key(
            Key::Esc,
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(escape_outcome, KeyOutcome::Continue);
        assert_eq!(typed_hint, "");

        let second_escape_outcome = handle_key(
            Key::Esc,
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(second_escape_outcome, KeyOutcome::Exit);
    }

    #[test]
    fn invalid_hint_exits_single_select_and_can_be_cleared_in_multi_select() {
        let matches = input_matches();
        let mut multi = false;
        let mut skip = 0;
        let mut typed_hint = String::new();
        let mut chosen = vec![];

        let single_outcome = handle_key(
            Key::Char('z'),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(single_outcome, KeyOutcome::Exit);
        assert_eq!(chosen, []);

        multi = true;
        typed_hint.clear();

        let multi_outcome = handle_key(
            Key::Char('z'),
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(multi_outcome, KeyOutcome::Continue);
        assert_eq!(typed_hint, "z");

        let escape_outcome = handle_key(
            Key::Esc,
            &mut multi,
            &mut skip,
            &mut typed_hint,
            &mut chosen,
            &matches,
            1,
        );

        assert_eq!(escape_outcome, KeyOutcome::Continue);
        assert_eq!(typed_hint, "");
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
            select_foreground_color: test_color("default"),
            select_background_color: test_color("default"),
            multi_foreground_color: test_color("default"),
            multi_background_color: test_color("default"),
            foreground_color: test_color("default"),
            background_color: test_color("default"),
            hint_background_color: test_color("default"),
            hint_foreground_color: test_color("default"),
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
    fn hint_column_places_all_positions() {
        assert_eq!(hint_column(HintPosition::Left, 10, 4, 2, false), 11);
        assert_eq!(hint_column(HintPosition::Right, 10, 4, 2, false), 13);
        assert_eq!(hint_column(HintPosition::OffLeft, 10, 4, 2, false), 9);
        assert_eq!(hint_column(HintPosition::OffRight, 10, 4, 2, false), 15);
        assert_eq!(hint_column(HintPosition::OffLeft, 10, 4, 2, true), 7);
    }

    #[test]
    fn render_writes_visible_output_to_provided_writer() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            default_colors(),
        );
        let mut output = Vec::new();

        view.render(&mut output, "").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("127.0.0.1"));
        assert!(rendered.contains("a"));
    }

    #[test]
    fn render_overlays_typed_hint_prefix() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            default_colors(),
        );
        let mut output = Vec::new();

        view.render(&mut output, "a").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert_eq!(rendered.matches('a').count(), 2);
    }

    #[test]
    fn render_uses_selected_match_color() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            ViewColors {
                select_foreground_color: test_color("red"),
                ..default_colors()
            },
        );
        let mut output = Vec::new();

        view.render(&mut output, "").unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains(&format!("{}", color::Fg(color::Red))));
    }

    #[test]
    fn render_returns_write_errors() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            default_colors(),
        );
        let mut output = FailingWriter;

        let error = view.render(&mut output, "").unwrap_err();

        assert_eq!(error.to_string(), "write failed");
    }

    #[test]
    fn render_returns_flush_errors() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            default_colors(),
        );
        let mut output = FlushFailingWriter;

        let error = view.render(&mut output, "").unwrap_err();

        assert_eq!(error.to_string(), "flush failed");
    }

    #[test]
    fn listen_returns_input_read_errors() {
        let lines = split("127.0.0.1");
        let custom = [].to_vec();
        let mut state = state::State::new(&lines, "abcd", &custom);
        let mut view = View::new(
            &mut state,
            default_options(HintPosition::Left, false),
            default_colors(),
        );
        let mut input = FailingReader;
        let mut output = Vec::new();

        let error = view.listen(&mut input, &mut output).unwrap_err();

        assert_eq!(error.to_string(), "read failed");
    }

    #[test]
    fn display_column_uses_unicode_width() {
        let line = "你λ 127.0.0.1";

        assert_eq!(display_column(line, "你λ ".len()), 4);
    }
}
