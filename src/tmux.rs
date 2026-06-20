use crate::tmux_options::{shell_quote, PickerArgs};
use crate::tmux_script::PaneScript;
use crate::tmux_selection::SelectionSet;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use clap::{Arg, ArgAction, ArgMatches, Command as ClapCommand};
use std::fmt;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutput {
    stdout: String,
    stderr: String,
    status: Option<i32>,
    success: bool,
}

impl CommandOutput {
    #[cfg(test)]
    fn success(stdout: String) -> CommandOutput {
        CommandOutput {
            stdout,
            stderr: String::new(),
            status: Some(0),
            success: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandError {
    command: Vec<String>,
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

impl CommandError {
    fn command_line(&self, include_sensitive: bool) -> String {
        if include_sensitive {
            return self.command.join(" ");
        }

        let mut command = self.command.clone();

        if command.first().map(String::as_str) == Some("bash")
            && command.get(1).map(String::as_str) == Some("-c")
            && command.get(4).is_some()
        {
            command[4] = "<selected text>".to_string();
        }

        command.join(" ")
    }

    fn format_with_command(&self, include_sensitive: bool) -> String {
        let status = self
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut message = format!(
            "command `{}` failed with status {}",
            self.command_line(include_sensitive),
            status
        );

        if !self.stdout.is_empty() {
            message.push_str(&format!(": stdout: {}", self.stdout));
        }

        if !self.stderr.is_empty() {
            message.push_str(&format!(": stderr: {}", self.stderr));
        }

        message
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_with_command(true))
    }
}

impl std::error::Error for CommandError {}

type CommandResult<T> = Result<T, CommandError>;
type OrchestrationResult<T> = Result<T, OrchestrationError>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunContext {
    run_id: String,
    result_path: String,
    signal: String,
    capture_signal: String,
    start_signal: String,
    active_pane_id: Option<String>,
    thumbs_pane_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OrchestrationError {
    Startup(String),
    Command {
        phase: &'static str,
        context: Box<RunContext>,
        source: Box<CommandError>,
    },
    Parse {
        phase: &'static str,
        context: Box<RunContext>,
        message: String,
    },
}

impl OrchestrationError {
    fn command(
        phase: &'static str,
        context: RunContext,
        source: CommandError,
    ) -> OrchestrationError {
        OrchestrationError::Command {
            phase,
            context: Box::new(context),
            source: Box::new(source),
        }
    }

    fn parse(phase: &'static str, context: RunContext, message: String) -> OrchestrationError {
        OrchestrationError::Parse {
            phase,
            context: Box::new(context),
            message,
        }
    }

    fn debug_message(&self) -> String {
        match self {
            OrchestrationError::Startup(message) => message.clone(),
            OrchestrationError::Command {
                phase,
                context,
                source,
            } => format!(
                "phase `{}` failed for run `{}`: {}\nresult_path: {}\nsignals: finished={}, captured={}, start={}\nactive_pane_id: {}\nthumbs_pane_id: {}",
                phase,
                context.run_id,
                source.format_with_command(true),
                context.result_path,
                context.signal,
                context.capture_signal,
                context.start_signal,
                context.active_pane_id.as_deref().unwrap_or("<unknown>"),
                context.thumbs_pane_id.as_deref().unwrap_or("<unknown>")
            ),
            OrchestrationError::Parse {
                phase,
                context,
                message,
            } => format!(
                "phase `{}` failed for run `{}`: {}\nresult_path: {}\nsignals: finished={}, captured={}, start={}\nactive_pane_id: {}\nthumbs_pane_id: {}",
                phase,
                context.run_id,
                message,
                context.result_path,
                context.signal,
                context.capture_signal,
                context.start_signal,
                context.active_pane_id.as_deref().unwrap_or("<unknown>"),
                context.thumbs_pane_id.as_deref().unwrap_or("<unknown>")
            ),
        }
    }
}

impl fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrchestrationError::Startup(message) => write!(f, "{}", message),
            OrchestrationError::Command { phase, source, .. } => {
                write!(
                    f,
                    "phase `{}` failed: {}",
                    phase,
                    source.format_with_command(false)
                )
            }
            OrchestrationError::Parse { phase, message, .. } => {
                write!(f, "phase `{}` failed: {}", phase, message)
            }
        }
    }
}

impl std::error::Error for OrchestrationError {}

trait Executor {
    fn execute(&mut self, args: Vec<String>) -> CommandResult<CommandOutput>;
}

struct RealShell {}

impl RealShell {
    fn new() -> RealShell {
        RealShell {}
    }
}

impl Executor for RealShell {
    fn execute(&mut self, args: Vec<String>) -> CommandResult<CommandOutput> {
        if args.is_empty() {
            return Err(CommandError {
                command: args,
                status: None,
                stdout: String::new(),
                stderr: "empty command".to_string(),
            });
        }

        let execution = Command::new(args[0].as_str())
            .args(&args[1..])
            .output()
            .map_err(|error| CommandError {
                command: args.clone(),
                status: None,
                stdout: String::new(),
                stderr: error.to_string(),
            })?;

        let output = CommandOutput {
            stdout: String::from_utf8_lossy(&execution.stdout)
                .trim_end()
                .to_string(),
            stderr: String::from_utf8_lossy(&execution.stderr)
                .trim_end()
                .to_string(),
            status: execution.status.code(),
            success: execution.status.success(),
        };

        if output.success {
            Ok(output)
        } else {
            Err(CommandError {
                command: args,
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectionOutcome {
    Executed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivePane {
    id: String,
    height: i32,
    scroll_position: Option<i32>,
    zoomed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneParseError(String);

impl fmt::Display for PaneParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ActivePane {
    fn parse_list_panes(output: &str) -> Result<ActivePane, PaneParseError> {
        for (line_index, line) in output.lines().enumerate() {
            if line.is_empty() {
                continue;
            }

            let fields = line.split('\t').collect::<Vec<_>>();
            let [pane_id, in_mode, height, scroll_position, zoomed, active] = fields.as_slice()
            else {
                return Err(PaneParseError(format!(
                    "malformed list-panes line {}: expected 6 tab-separated fields, got {}",
                    line_index + 1,
                    fields.len()
                )));
            };

            if *active != "active" {
                continue;
            }

            let in_mode = parse_tmux_flag(in_mode, "pane_in_mode", line_index + 1)?;
            let height = height.parse().map_err(|_| {
                PaneParseError(format!(
                    "invalid pane_height on list-panes line {}: {}",
                    line_index + 1,
                    height
                ))
            })?;
            let zoomed = parse_tmux_flag(zoomed, "window_zoomed_flag", line_index + 1)?;
            let scroll_position = if in_mode {
                Some(scroll_position.parse().map_err(|_| {
                    PaneParseError(format!(
                        "invalid scroll_position on list-panes line {}: {}",
                        line_index + 1,
                        scroll_position
                    ))
                })?)
            } else {
                None
            };

            return Ok(ActivePane {
                id: pane_id.to_string(),
                height,
                scroll_position,
                zoomed,
            });
        }

        Err(PaneParseError(
            "missing active pane in tmux list-panes output".to_string(),
        ))
    }
}

fn parse_tmux_flag(value: &str, name: &str, line_number: usize) -> Result<bool, PaneParseError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(PaneParseError(format!(
            "invalid {} on list-panes line {}: {}",
            name, line_number, value
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThumbsPane {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunSignals {
    finished: String,
    captured: String,
    start: String,
}

struct Swapper<'a> {
    executor: &'a mut dyn Executor,
    dir: String,
    command: String,
    upcase_command: String,
    multi_command: String,
    osc52: bool,
    content: Option<String>,
    signals: RunSignals,
    run_id: String,
    result_path: String,
    active_pane_id: Option<String>,
    thumbs_pane_id: Option<String>,
}

impl<'a> Swapper<'a> {
    fn new(
        executor: &'a mut dyn Executor,
        dir: String,
        command: String,
        upcase_command: String,
        multi_command: String,
        osc52: bool,
    ) -> Swapper<'a> {
        let since_the_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let signal_id = format!(
            "{}-{}-{}",
            since_the_epoch.as_secs(),
            since_the_epoch.subsec_nanos(),
            sequence
        );
        let signals = RunSignals {
            finished: format!("thumbs-finished-{}", signal_id),
            captured: format!("thumbs-captured-{}", signal_id),
            start: format!("thumbs-start-{}", signal_id),
        };
        let result_path = std::env::temp_dir()
            .join(format!("thumbs-last-{}", signal_id))
            .to_string_lossy()
            .into_owned();

        Swapper {
            executor,
            dir,
            command,
            upcase_command,
            multi_command,
            osc52,
            content: None,
            signals,
            run_id: signal_id,
            result_path,
            active_pane_id: None,
            thumbs_pane_id: None,
        }
    }

    fn run_context(&self) -> RunContext {
        RunContext {
            run_id: self.run_id.clone(),
            result_path: self.result_path.clone(),
            signal: self.signals.finished.clone(),
            capture_signal: self.signals.captured.clone(),
            start_signal: self.signals.start.clone(),
            active_pane_id: self.active_pane_id.clone(),
            thumbs_pane_id: self.thumbs_pane_id.clone(),
        }
    }

    fn command_error(&self, phase: &'static str, error: CommandError) -> OrchestrationError {
        OrchestrationError::command(phase, self.run_context(), error)
    }

    fn run(&mut self) -> OrchestrationResult<SelectionOutcome> {
        let active_pane = self.capture_active_pane()?;
        let thumbs_pane = self.execute_thumbs(&active_pane)?;

        self.swap_panes(&active_pane, &thumbs_pane)?;
        self.resize_pane(&active_pane, &thumbs_pane)?;
        self.start_thumbs()?;
        self.wait_thumbs()?;
        self.retrieve_content()?;
        self.destroy_content()?;
        self.execute_command()
    }

    fn capture_active_pane(&mut self) -> OrchestrationResult<ActivePane> {
        let active_command = [
      "tmux",
      "list-panes",
      "-F",
      "#{pane_id}\t#{?pane_in_mode,1,0}\t#{pane_height}\t#{scroll_position}\t#{window_zoomed_flag}\t#{?pane_active,active,nope}",
    ];

        let output = self
            .executor
            .execute(active_command.iter().map(|arg| arg.to_string()).collect())
            .map_err(|error| self.command_error("capture_active_pane", error))?
            .stdout;

        let active_pane = ActivePane::parse_list_panes(&output).map_err(|error| {
            OrchestrationError::parse("capture_active_pane", self.run_context(), error.to_string())
        })?;

        self.active_pane_id = Some(active_pane.id.clone());

        Ok(active_pane)
    }

    fn execute_thumbs(&mut self, active_pane: &ActivePane) -> OrchestrationResult<ThumbsPane> {
        let options_command = ["tmux", "show", "-g"];
        let params: Vec<String> = options_command.iter().map(|arg| arg.to_string()).collect();
        let options = self
            .executor
            .execute(params)
            .map_err(|error| self.command_error("start_picker", error))?
            .stdout;
        let args = PickerArgs::from_tmux_options(&options)
            .map_err(|message| {
                OrchestrationError::parse("parse_picker_options", self.run_context(), message)
            })?
            .render_shell_args();

        let pane_command = PaneScript {
            dir: &self.dir,
            result_path: &self.result_path,
            picker_args: &args,
            active_pane_id: &active_pane.id,
            active_pane_height: active_pane.height,
            active_pane_scroll_position: active_pane.scroll_position,
            active_pane_zoomed: active_pane.zoomed,
            finished_signal: &self.signals.finished,
            captured_signal: &self.signals.captured,
            start_signal: &self.signals.start,
        }
        .render();

        let thumbs_command = [
            "tmux",
            "new-window",
            "-P",
            "-F",
            "#{pane_id}",
            "-d",
            "-n",
            "[thumbs]",
            pane_command.as_str(),
        ];

        let params: Vec<String> = thumbs_command.iter().map(|arg| arg.to_string()).collect();

        let thumbs_pane = ThumbsPane {
            id: self
                .executor
                .execute(params)
                .map_err(|error| self.command_error("start_picker", error))?
                .stdout,
        };
        self.thumbs_pane_id = Some(thumbs_pane.id.clone());
        self.wait_capture()?;

        Ok(thumbs_pane)
    }

    fn swap_panes(
        &mut self,
        active_pane: &ActivePane,
        thumbs_pane: &ThumbsPane,
    ) -> OrchestrationResult<()> {
        let swap_command = [
            "tmux",
            "swap-pane",
            "-d",
            "-s",
            active_pane.id.as_str(),
            "-t",
            thumbs_pane.id.as_str(),
        ];

        let params = swap_command
            .iter()
            .filter(|&s| !s.is_empty())
            .map(|arg| arg.to_string())
            .collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("swap_panes", error))?;

        Ok(())
    }

    fn resize_pane(
        &mut self,
        active_pane: &ActivePane,
        thumbs_pane: &ThumbsPane,
    ) -> OrchestrationResult<()> {
        if !active_pane.zoomed {
            return Ok(());
        }

        let resize_command = ["tmux", "resize-pane", "-t", thumbs_pane.id.as_str(), "-Z"];

        let params = resize_command
            .iter()
            .filter(|&s| !s.is_empty())
            .map(|arg| arg.to_string())
            .collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("resize_pane", error))?;

        Ok(())
    }

    fn wait_capture(&mut self) -> OrchestrationResult<()> {
        let wait_command = ["tmux", "wait-for", self.signals.captured.as_str()];
        let params = wait_command.iter().map(|arg| arg.to_string()).collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("wait_capture", error))?;

        Ok(())
    }

    fn start_thumbs(&mut self) -> OrchestrationResult<()> {
        let start_command = ["tmux", "wait-for", "-S", self.signals.start.as_str()];
        let params = start_command.iter().map(|arg| arg.to_string()).collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("start_thumbs", error))?;

        Ok(())
    }

    fn wait_thumbs(&mut self) -> OrchestrationResult<()> {
        let wait_command = ["tmux", "wait-for", self.signals.finished.as_str()];
        let params = wait_command.iter().map(|arg| arg.to_string()).collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("wait_thumbs", error))?;

        Ok(())
    }

    fn retrieve_content(&mut self) -> OrchestrationResult<()> {
        let retrieve_command = ["cat", self.result_path.as_str()];
        let params = retrieve_command.iter().map(|arg| arg.to_string()).collect();

        match self.executor.execute(params) {
            Ok(output) => self.content = Some(output.stdout),
            Err(error) if missing_result_file_error(&error) => self.content = None,
            Err(error) => return Err(self.command_error("read_selection", error)),
        }

        Ok(())
    }

    fn destroy_content(&mut self) -> OrchestrationResult<()> {
        let retrieve_command = ["rm", "-f", self.result_path.as_str()];
        let params = retrieve_command.iter().map(|arg| arg.to_string()).collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("cleanup_result", error))?;

        Ok(())
    }

    fn execute_command(&mut self) -> OrchestrationResult<SelectionOutcome> {
        let Some(content) = self.content.clone() else {
            return Ok(SelectionOutcome::Cancelled);
        };

        if content.is_empty() {
            return Ok(SelectionOutcome::Cancelled);
        }

        let selections = SelectionSet::parse(&content).map_err(|error| {
            OrchestrationError::parse("parse_selection", self.run_context(), error.to_string())
        })?;

        if selections.is_empty() {
            return Ok(SelectionOutcome::Cancelled);
        }

        if selections.is_multi() {
            let text = selections.multi_text();
            self.execute_final_command(&text, &self.multi_command.clone())?;

            return Ok(SelectionOutcome::Executed);
        }

        if let Some(selection) = selections.single() {
            if self.osc52 {
                let base64_text = BASE64_STANDARD.encode(selection.text.as_bytes());
                let osc_seq = format!("\x1b]52;0;{}\x07", base64_text);
                let tmux_seq = format!("\x1bPtmux;{}\x1b\\", osc_seq.replace("\x1b", "\x1b\x1b"));

                // When termion drops the alternate screen, tmux marks the pane for redraw.
                // If we print the OSC copy escape sequence before the redraw is completed,
                //    tmux will *not* send the sequence to the host terminal. See the following
                //    call chain in tmux: `input_dcs_dispatch` -> `screen_write_rawstring`
                //    -> `tty_write` -> `tty_client_ready`. In this case, `tty_client_ready`
                //    will return false, thus preventing the escape sequence from being sent.
                // Therefore, wait a little bit here for the redraw to finish.
                std::thread::sleep(std::time::Duration::from_millis(100));

                std::io::stdout().write_all(tmux_seq.as_bytes()).unwrap();
                std::io::stdout().flush().unwrap();
            }

            let execute_command = if selection.upcase {
                self.upcase_command.clone()
            } else {
                self.command.clone()
            };

            // The command we run has two arguments:
            //  * The first arg is the (trimmed) text. This gets stored in a variable, in order to
            //    preserve quoting and special characters.
            //
            //  * The second argument is the user's command, with the '{}' token replaced with an
            //    unquoted reference to the variable containing the text.
            //
            // The reference is unquoted, unfortunately, because the token may already have been
            // spliced into a string (e.g 'tmux display-message "Copied {}"'), and it's impossible (or
            // at least exceedingly difficult) to determine the correct quoting level.
            //
            // The alternative of literally splicing the text into the command is bad and it causes all
            // kinds of harmful escaping issues that the user cannot reasonably avoid.
            //
            // For example, imagine some pattern matched the text "foo;rm *" and the user's command was
            // an innocuous "echo {}". With literal splicing, we would run the command "echo foo;rm *".
            // That's BAD. Without splicing, instead we execute "echo ${THUMB}" which does mostly the
            // right thing regardless the contents of the text. (At worst, bash will word-separate the
            // unquoted variable; but it won't _execute_ those words in common scenarios).
            //
            // Ideally user commands would just use "${THUMB}" to begin with rather than having any
            // sort of ad-hoc string splicing here at all, and then they could specify the quoting they
            // want, but that would break backwards compatibility.
            self.execute_final_command(selection.text.trim_end(), &execute_command)?;

            return Ok(SelectionOutcome::Executed);
        }

        Ok(SelectionOutcome::Cancelled)
    }

    fn execute_final_command(
        &mut self,
        text: &str,
        execute_command: &str,
    ) -> OrchestrationResult<()> {
        let final_command = str::replace(execute_command, "{}", "${THUMB}");
        let retrieve_command = [
            "bash",
            "-c",
            "THUMB=\"$1\"; eval \"$2\"",
            "--",
            text,
            final_command.as_str(),
        ];

        let params = retrieve_command.iter().map(|arg| arg.to_string()).collect();

        self.executor
            .execute(params)
            .map_err(|error| self.command_error("execute_command", error))?;

        Ok(())
    }
}

fn missing_result_file_error(error: &CommandError) -> bool {
    error.command.first().map(String::as_str) == Some("cat")
        && error
            .command
            .get(1)
            .map(|path| !std::path::Path::new(path).exists())
            .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwapperOptions {
    dir: String,
    command: String,
    upcase_command: String,
    multi_command: String,
    osc52: bool,
}

impl SwapperOptions {
    fn from_matches(args: &ArgMatches) -> OrchestrationResult<SwapperOptions> {
        let options = SwapperOptions {
            dir: required_tmux_string(args, "dir")?.to_string(),
            command: required_tmux_string(args, "command")?.to_string(),
            upcase_command: required_tmux_string(args, "upcase_command")?.to_string(),
            multi_command: required_tmux_string(args, "multi_command")?.to_string(),
            osc52: args.get_flag("osc52"),
        };

        if options.dir.is_empty() {
            return Err(OrchestrationError::Startup(
                "Invalid tmux-thumbs execution. Are you trying to execute tmux-thumbs directly?"
                    .to_string(),
            ));
        }

        Ok(options)
    }
}

fn required_tmux_string<'a>(
    args: &'a ArgMatches,
    name: &'static str,
) -> OrchestrationResult<&'a str> {
    args.get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| OrchestrationError::Startup(format!("missing required option `{}`", name)))
}

fn app() -> ClapCommand {
    ClapCommand::new("tmux-thumbs")
    .version(env!("CARGO_PKG_VERSION"))
    .about("A lightning fast version of tmux-fingers, copy/pasting tmux like vimium/vimperator")
    .arg(
      Arg::new("dir")
        .help("Directory where to execute thumbs")
        .long("dir")
        .default_value(""),
    )
    .arg(
      Arg::new("command")
        .help("Command to execute after choose a hint")
        .long("command")
        .default_value("tmux set-buffer -- \"{}\" && tmux display-message \"Copied {}\""),
    )
    .arg(
      Arg::new("upcase_command")
        .help("Command to execute after choose a hint, in upcase")
        .long("upcase-command")
        .default_value("tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Copied {}\""),
    )
    .arg(
      Arg::new("multi_command")
        .help("Command to execute after choose multiple hints")
        .long("multi-command")
        .default_value("tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Multi copied {}\""),
    )
    .arg(
      Arg::new("osc52")
        .help("Print OSC52 copy escape sequence in addition to running the pick command")
        .long("osc52")
        .short('o')
        .action(ArgAction::SetTrue),
    )
}

fn app_args() -> ArgMatches {
    app().get_matches()
}

fn run() -> OrchestrationResult<SelectionOutcome> {
    let args = app_args();
    let options = SwapperOptions::from_matches(&args)?;

    let mut executor = RealShell::new();
    let mut swapper = Swapper::new(
        &mut executor,
        options.dir,
        options.command,
        options.upcase_command,
        options.multi_command,
        options.osc52,
    );

    swapper.run()
}

pub fn main() {
    if let Err(error) = run() {
        if debug_enabled() {
            eprintln!("{}", error.debug_message());
        } else {
            eprintln!("{}", error);
        }

        std::process::exit(1);
    }
}

fn debug_enabled() -> bool {
    std::env::var("THUMBS_DEBUG")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestShell {
        outputs: Vec<CommandResult<CommandOutput>>,
        executed: Option<Vec<String>>,
        executions: Vec<Vec<String>>,
    }

    impl TestShell {
        fn new(outputs: Vec<String>) -> TestShell {
            TestShell::new_results(
                outputs
                    .into_iter()
                    .map(CommandOutput::success)
                    .map(Ok)
                    .collect(),
            )
        }

        fn new_results(outputs: Vec<CommandResult<CommandOutput>>) -> TestShell {
            TestShell {
                executed: None,
                outputs,
                executions: vec![],
            }
        }

        fn last_executed(&self) -> Option<Vec<String>> {
            self.executed.clone()
        }

        fn new_window_command(&self) -> Option<&[String]> {
            self.executions
                .iter()
                .find(|command| command.get(1) == Some(&"new-window".to_string()))
                .map(Vec::as_slice)
        }
    }

    impl Executor for TestShell {
        fn execute(&mut self, args: Vec<String>) -> CommandResult<CommandOutput> {
            self.executed = Some(args.clone());
            self.executions.push(args.clone());

            match self.outputs.pop().unwrap() {
                Ok(output) => Ok(output),
                Err(mut error) => {
                    if error.command.is_empty() {
                        error.command = args;
                    }

                    Err(error)
                }
            }
        }
    }

    #[test]
    fn swapper_options_parse_clap_defaults() {
        let matches = app()
            .try_get_matches_from(["tmux-thumbs", "--dir", "/plugin"])
            .unwrap();
        let options = SwapperOptions::from_matches(&matches).unwrap();

        assert_eq!(options.dir, "/plugin");
        assert_eq!(
            options.command,
            "tmux set-buffer -- \"{}\" && tmux display-message \"Copied {}\""
        );
        assert_eq!(
            options.upcase_command,
            "tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Copied {}\""
        );
        assert_eq!(
            options.multi_command,
            "tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Multi copied {}\""
        );
        assert!(!options.osc52);
    }

    #[test]
    fn swapper_options_parse_custom_commands_and_osc52() {
        let matches = app()
            .try_get_matches_from([
                "tmux-thumbs",
                "--dir",
                "/plugin",
                "--command",
                "copy {}",
                "--upcase-command",
                "paste {}",
                "--multi-command",
                "multi {}",
                "--osc52",
            ])
            .unwrap();
        let options = SwapperOptions::from_matches(&matches).unwrap();

        assert_eq!(options.dir, "/plugin");
        assert_eq!(options.command, "copy {}");
        assert_eq!(options.upcase_command, "paste {}");
        assert_eq!(options.multi_command, "multi {}");
        assert!(options.osc52);
    }

    #[test]
    fn swapper_options_reject_empty_dir() {
        let matches = app().try_get_matches_from(["tmux-thumbs"]).unwrap();
        let error = SwapperOptions::from_matches(&matches).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Invalid tmux-thumbs execution. Are you trying to execute tmux-thumbs directly?"
        );
    }

    #[test]
    fn retrieve_active_pane() {
        let last_command_outputs = vec![
            "%97\t0\t24\t1\t0\tactive\n%106\t0\t24\t1\t0\tnope\n%107\t0\t24\t1\t0\tnope\n"
                .to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();

        assert_eq!(active_pane.id, "%97");
        assert_eq!(active_pane.height, 24);
        assert_eq!(active_pane.scroll_position, None);
        assert!(!active_pane.zoomed);
    }

    #[test]
    fn parses_copy_mode_active_pane() {
        let active_pane = ActivePane::parse_list_panes("%97\t1\t24\t3\t1\tactive\n").unwrap();

        assert_eq!(active_pane.id, "%97");
        assert_eq!(active_pane.height, 24);
        assert_eq!(active_pane.scroll_position, Some(3));
        assert!(active_pane.zoomed);
    }

    #[test]
    fn active_pane_parser_reports_missing_active_pane() {
        let error = ActivePane::parse_list_panes("%97\t0\t24\t0\t0\tnope\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "missing active pane in tmux list-panes output"
        );
    }

    #[test]
    fn active_pane_parser_reports_malformed_line() {
        let error = ActivePane::parse_list_panes("%97\t0\t24\n").unwrap_err();

        assert!(error.to_string().contains("malformed list-panes line 1"));
    }

    #[test]
    fn active_pane_parser_reports_invalid_height() {
        let error = ActivePane::parse_list_panes("%97\t0\ttall\t0\t0\tactive\n").unwrap_err();

        assert!(error.to_string().contains("invalid pane_height"));
    }

    #[test]
    fn active_pane_parser_reports_invalid_scroll_position() {
        let error = ActivePane::parse_list_panes("%97\t1\t24\tfar\t0\tactive\n").unwrap_err();

        assert!(error.to_string().contains("invalid scroll_position"));
    }

    #[test]
    fn active_pane_parser_reports_invalid_zoom_flag() {
        let error = ActivePane::parse_list_panes("%97\t0\t24\t0\tmaybe\tactive\n").unwrap_err();

        assert!(error.to_string().contains("invalid window_zoomed_flag"));
    }

    #[test]
    fn shell_quote_escapes_shell_values() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("bright green"), "'bright green'");
        assert_eq!(shell_quote("quote'color"), "'quote'\\''color'");
        assert_eq!(shell_quote("bright\"blue"), "'bright\"blue'");
        assert_eq!(shell_quote("slash\\color"), "'slash\\color'");
    }

    #[test]
    fn picker_args_parse_true_boolean_values() {
        let args = PickerArgs::from_tmux_options(
            "@thumbs-reverse enabled\n@thumbs-unique 1\n@thumbs-contrast true\n",
        )
        .unwrap()
        .render_shell_args();

        assert_eq!(args, ["--reverse", "--unique", "--contrast"]);
    }

    #[test]
    fn picker_args_ignore_false_and_empty_boolean_values() {
        let args = PickerArgs::from_tmux_options(
            "@thumbs-reverse 0\n@thumbs-unique disabled\n@thumbs-contrast \"\"\n",
        )
        .unwrap()
        .render_shell_args();

        assert!(args.is_empty());
    }

    #[test]
    fn picker_args_parse_quoted_strings_and_regex_backslashes() {
        let args = PickerArgs::from_tmux_options(
            "@thumbs-fg-color green\n@thumbs-regexp-1 \"foo\\\\d+'bar\"\n",
        )
        .unwrap()
        .render_shell_args();

        assert_eq!(
            args,
            ["--fg-color", "'green'", "--regexp", "'foo\\d+'\\''bar'",]
        );
    }

    #[test]
    fn picker_args_validate_typed_values() {
        let args = PickerArgs::from_tmux_options(
            "@thumbs-alphabet numeric\n@thumbs-position off_right\n@thumbs-fg-color #1b1cbf\n",
        )
        .unwrap()
        .render_shell_args();

        assert_eq!(
            args,
            [
                "--alphabet",
                "'numeric'",
                "--position",
                "'off_right'",
                "--fg-color",
                "'#1b1cbf'",
            ]
        );
    }

    #[test]
    fn picker_args_reject_invalid_position() {
        let error = PickerArgs::from_tmux_options("@thumbs-position center\n").unwrap_err();

        assert_eq!(error, "Unknown hint position: center");
    }

    #[test]
    fn picker_args_reject_invalid_color() {
        let error = PickerArgs::from_tmux_options("@thumbs-fg-color wat\n").unwrap_err();

        assert_eq!(error, "Unknown color: wat");
    }

    #[test]
    fn picker_args_reject_invalid_alphabet() {
        let error = PickerArgs::from_tmux_options("@thumbs-alphabet wat\n").unwrap_err();

        assert_eq!(error, "Unknown alphabet: wat");
    }

    #[test]
    fn picker_args_reject_invalid_regexp() {
        let error = PickerArgs::from_tmux_options("@thumbs-regexp-1 [\n").unwrap_err();

        assert!(error.contains("Invalid custom regexp"));
    }

    #[test]
    fn malformed_active_pane_output_returns_phase_error() {
        let last_command_outputs = vec!["%97\t0\t24\n".to_string()];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let error = swapper.capture_active_pane().unwrap_err();

        let OrchestrationError::Parse { phase, message, .. } = &error else {
            panic!("expected parse error");
        };

        assert_eq!(*phase, "capture_active_pane");
        assert!(message.contains("malformed list-panes line 1"));
        assert!(error.to_string().contains("phase `capture_active_pane`"));
    }

    #[test]
    fn failing_tmux_command_returns_command_error() {
        let last_command_outputs = vec![Err(CommandError {
            command: vec![],
            status: Some(1),
            stdout: "partial output".to_string(),
            stderr: "tmux failed".to_string(),
        })];
        let mut executor = TestShell::new_results(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let error = swapper.capture_active_pane().unwrap_err();

        let OrchestrationError::Command {
            phase,
            context,
            source,
        } = &error
        else {
            panic!("expected command error");
        };

        assert_eq!(*phase, "capture_active_pane");
        assert_eq!(source.command[0], "tmux");
        assert_eq!(source.status, Some(1));
        assert_eq!(source.stdout, "partial output");
        assert_eq!(source.stderr, "tmux failed");
        assert!(context.result_path.contains("thumbs-last-"));
        assert!(error.to_string().contains("phase `capture_active_pane`"));
        assert!(error.to_string().contains("tmux list-panes"));
        assert!(error.to_string().contains("status 1"));
        assert!(error.to_string().contains("stdout: partial output"));
        assert!(error.to_string().contains("stderr: tmux failed"));
        assert!(error.debug_message().contains(context.run_id.as_str()));
        assert!(error.debug_message().contains(context.result_path.as_str()));
        assert!(error.debug_message().contains(context.signal.as_str()));
    }

    #[test]
    fn swap_panes() {
        let last_command_outputs = vec![
            "".to_string(),
            "".to_string(),
            "%100".to_string(),
            "".to_string(),
            "%106\t0\t24\t1\t0\tnope\n%98\t0\t24\t1\t0\tactive\n%107\t0\t24\t1\t0\tnope\n"
                .to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        let thumbs_pane = swapper.execute_thumbs(&active_pane).unwrap();
        swapper.swap_panes(&active_pane, &thumbs_pane).unwrap();

        let expectation = ["tmux", "swap-pane", "-d", "-s", "%98", "-t", "%100"];

        assert_eq!(executor.last_executed().unwrap(), expectation);
    }

    #[test]
    fn waits_for_capture_before_swapping_split_panes() {
        let last_command_outputs = vec![
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "%100".to_string(),
            "".to_string(),
            "%106\t0\t12\t0\t0\tnope\n%98\t0\t8\t0\t0\tactive\n%107\t0\t12\t0\t0\tnope\n"
                .to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        let thumbs_pane = swapper.execute_thumbs(&active_pane).unwrap();
        swapper.swap_panes(&active_pane, &thumbs_pane).unwrap();
        swapper.start_thumbs().unwrap();

        let wait_for_capture_index = executor
            .executions
            .iter()
            .position(|command| {
                command.len() == 3
                    && command[0] == "tmux"
                    && command[1] == "wait-for"
                    && command[2].starts_with("thumbs-captured-")
            })
            .expect("capture must complete before swapping panes");

        let swap_index = executor
            .executions
            .iter()
            .position(|command| {
                command.as_slice() == ["tmux", "swap-pane", "-d", "-s", "%98", "-t", "%100"]
            })
            .expect("test setup should swap panes");

        assert!(wait_for_capture_index < swap_index);

        let start_index = executor
            .executions
            .iter()
            .position(|command| {
                command.len() == 4
                    && command[0] == "tmux"
                    && command[1] == "wait-for"
                    && command[2] == "-S"
                    && command[3].starts_with("thumbs-start-")
            })
            .expect("thumbs should start after the pane is swapped into place");

        assert!(swap_index < start_index);

        let new_window_command = executor
            .executions
            .iter()
            .find(|command| command.get(1) == Some(&"new-window".to_string()))
            .expect("test setup should create a thumbs window");
        let pane_command = new_window_command.last().unwrap();

        let trap_index = pane_command
            .find("trap '")
            .expect("script should install EXIT trap");
        let capture_index = pane_command
            .find("capture=\"$(tmux capture-pane -J -t '%98' -p | tail -n 8)\"")
            .unwrap();
        let capture_signal_index = pane_command
            .find("tmux wait-for -S 'thumbs-captured-")
            .unwrap();
        let start_wait_index = pane_command.find("tmux wait-for 'thumbs-start-").unwrap();
        let thumbs_index = pane_command.find("/target/release/thumbs").unwrap();
        let restore_index = pane_command.find("tmux swap-pane -t '%98'").unwrap();
        let finished_signal_index = pane_command
            .rfind("tmux wait-for -S 'thumbs-finished-")
            .unwrap();

        assert!(trap_index < capture_index);
        assert!(capture_index < capture_signal_index);
        assert!(capture_signal_index < start_wait_index);
        assert!(start_wait_index < thumbs_index);
        assert!(thumbs_index < restore_index);
        assert!(restore_index < finished_signal_index);
    }

    #[test]
    fn quoted_execution() {
        let last_command_outputs =
            vec!["Blah blah blah, the ignored user script output".to_string()];
        let mut executor = TestShell::new(last_command_outputs);

        let user_command = "echo \"{}\"".to_string();
        let upcase_command = "open \"{}\"".to_string();
        let multi_command = "open \"{}\"".to_string();
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            user_command,
            upcase_command,
            multi_command,
            false,
        );

        swapper.content = Some(format!(
            "{do_upcase}:{thumb_text}",
            do_upcase = false,
            thumb_text = "foobar;rm *",
        ));
        let outcome = swapper.execute_command().unwrap();

        let expectation = [
            "bash",
            // The actual shell command:
            "-c",
            "THUMB=\"$1\"; eval \"$2\"",
            // $0: The non-existent program name.
            "--",
            // $1: The value assigned to THUMB above.
            //     Not interpreted as a shell expression!
            "foobar;rm *",
            // $2: The user script, with {} replaced with ${THUMB},
            //     and will be eval'd with THUMB in scope.
            "echo \"${THUMB}\"",
        ];

        assert_eq!(executor.last_executed().unwrap(), expectation);
        assert_eq!(outcome, SelectionOutcome::Executed);
    }

    #[test]
    fn failing_final_command_returns_command_error() {
        let last_command_outputs = vec![Err(CommandError {
            command: vec![],
            status: Some(2),
            stdout: String::new(),
            stderr: "bash failed".to_string(),
        })];
        let mut executor = TestShell::new_results(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("false:foobar".to_string());

        let error = swapper.execute_command().unwrap_err();

        let OrchestrationError::Command { phase, source, .. } = &error else {
            panic!("expected command error");
        };

        assert_eq!(*phase, "execute_command");
        assert_eq!(source.command[0], "bash");
        assert_eq!(source.status, Some(2));
        assert_eq!(source.stderr, "bash failed");
        assert!(error.to_string().contains("phase `execute_command`"));
        assert!(!error.to_string().contains("foobar"));
        assert!(error.debug_message().contains("foobar"));
    }

    #[test]
    fn no_selection_does_not_execute_command() {
        let last_command_outputs = vec![];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("".to_string());
        let outcome = swapper.execute_command().unwrap();

        assert_eq!(executor.last_executed(), None);
        assert_eq!(outcome, SelectionOutcome::Cancelled);
    }

    #[test]
    fn malformed_selection_returns_parse_error() {
        let last_command_outputs = vec![];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("not-a-selection".to_string());
        let error = swapper.execute_command().unwrap_err();

        let OrchestrationError::Parse { phase, message, .. } = &error else {
            panic!("expected parse error");
        };

        assert_eq!(executor.last_executed(), None);
        assert_eq!(*phase, "parse_selection");
        assert!(message.contains("malformed selection line 1"));
        assert!(error.to_string().contains("phase `parse_selection`"));
    }

    #[test]
    fn malformed_multi_selection_returns_parse_error() {
        let last_command_outputs = vec![];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("false:first\nnot-a-selection".to_string());
        let error = swapper.execute_command().unwrap_err();

        let OrchestrationError::Parse { phase, message, .. } = &error else {
            panic!("expected parse error");
        };

        assert_eq!(executor.last_executed(), None);
        assert_eq!(*phase, "parse_selection");
        assert!(message.contains("malformed selection line 2"));
    }

    #[test]
    fn selected_text_may_contain_colons() {
        let last_command_outputs = vec!["".to_string()];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("false:https://example.com/a:b".to_string());
        let outcome = swapper.execute_command().unwrap();

        assert_eq!(
            executor.last_executed().unwrap()[4],
            "https://example.com/a:b"
        );
        assert_eq!(outcome, SelectionOutcome::Executed);
    }

    #[test]
    fn multi_selection_uses_multi_command_with_space_joined_text() {
        let last_command_outputs = vec!["".to_string()];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.content = Some("false:first\ntrue:https://example.com/a:b".to_string());
        let outcome = swapper.execute_command().unwrap();
        let executed = executor.last_executed().unwrap();

        assert_eq!(executed[4], "first https://example.com/a:b");
        assert_eq!(executed[5], "multi ${THUMB}");
        assert_eq!(outcome, SelectionOutcome::Executed);
    }

    #[test]
    fn picker_target_matches_retrieval_path() {
        let last_command_outputs = vec![
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "%100".to_string(),
            "".to_string(),
            "%98\t0\t8\t0\t0\tactive\n".to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "/plugin".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        swapper.execute_thumbs(&active_pane).unwrap();
        let result_path = swapper.result_path.clone();
        swapper.retrieve_content().unwrap();
        swapper.destroy_content().unwrap();

        let new_window_command = executor
            .new_window_command()
            .expect("thumbs window should be created");
        let pane_command = new_window_command.last().unwrap();

        assert_ne!(result_path, "/tmp/thumbs-last");
        assert!(pane_command.contains(&format!(" -t '{}' ", result_path)));
        assert!(executor
            .executions
            .iter()
            .any(|command| command.as_slice() == ["cat", result_path.as_str()]));
        assert!(executor
            .executions
            .iter()
            .any(|command| command.as_slice() == ["rm", "-f", result_path.as_str()]));
    }

    #[test]
    fn result_paths_are_unique_per_swapper() {
        let first_path = {
            let mut executor = TestShell::new(vec![]);
            Swapper::new(
                &mut executor,
                "/plugin".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                false,
            )
            .result_path
            .clone()
        };
        let second_path = {
            let mut executor = TestShell::new(vec![]);
            Swapper::new(
                &mut executor,
                "/plugin".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
                false,
            )
            .result_path
            .clone()
        };

        assert_ne!(first_path, second_path);
        assert!(first_path.contains("thumbs-last-"));
        assert!(second_path.contains("thumbs-last-"));
    }

    #[test]
    fn missing_result_file_content_does_not_execute_command() {
        let last_command_outputs = vec![Err(CommandError {
            command: vec![],
            status: Some(1),
            stdout: String::new(),
            stderr: "cat: missing: No such file or directory".to_string(),
        })];
        let mut executor = TestShell::new_results(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "".to_string(),
            "echo {}".to_string(),
            "open {}".to_string(),
            "multi {}".to_string(),
            false,
        );

        swapper.retrieve_content().unwrap();
        let outcome = swapper.execute_command().unwrap();

        assert_eq!(executor.executions.len(), 1);
        assert_eq!(executor.executions[0][0], "cat");
        assert_eq!(outcome, SelectionOutcome::Cancelled);
    }

    #[test]
    fn forwards_tmux_options_with_spaces() {
        let last_command_outputs = vec![
            "".to_string(),
            "%100".to_string(),
            "@thumbs-regexp-1 \"bright green\"\n".to_string(),
            "%98\t0\t8\t0\t0\tactive\n".to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "/plugin".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        swapper.execute_thumbs(&active_pane).unwrap();

        let new_window_command = executor
            .new_window_command()
            .expect("thumbs window should be created");
        let pane_command = new_window_command.last().unwrap();

        assert!(pane_command.contains("--regexp 'bright green'"));
    }

    #[test]
    fn quotes_tmux_options_with_shell_metacharacters() {
        let last_command_outputs = vec![
            "".to_string(),
            "%100".to_string(),
            "@thumbs-fg-color #1b1cbf\n@thumbs-bg-color blue\n@thumbs-regexp-1 \"foo'bar\"\n"
                .to_string(),
            "%98\t0\t8\t0\t0\tactive\n".to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "/plugin".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        swapper.execute_thumbs(&active_pane).unwrap();

        let new_window_command = executor
            .new_window_command()
            .expect("thumbs window should be created");
        let pane_command = new_window_command.last().unwrap();

        assert!(pane_command.contains("--fg-color '#1b1cbf'"));
        assert!(pane_command.contains("--bg-color 'blue'"));
        assert!(pane_command.contains("--regexp 'foo'\\''bar'"));
    }

    #[test]
    fn boolean_tmux_options_only_forward_true_values() {
        let last_command_outputs = vec![
            "".to_string(),
            "%100".to_string(),
            "@thumbs-reverse \"0\"\n@thumbs-unique disabled\n@thumbs-contrast enabled\n"
                .to_string(),
            "%98\t0\t8\t0\t0\tactive\n".to_string(),
        ];
        let mut executor = TestShell::new(last_command_outputs);
        let mut swapper = Swapper::new(
            &mut executor,
            "/plugin".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            false,
        );

        let active_pane = swapper.capture_active_pane().unwrap();
        swapper.execute_thumbs(&active_pane).unwrap();

        let new_window_command = executor
            .new_window_command()
            .expect("thumbs window should be created");
        let pane_command = new_window_command.last().unwrap();

        assert!(!pane_command.contains("--reverse"));
        assert!(!pane_command.contains("--unique"));
        assert!(pane_command.contains("--contrast"));
    }
}
