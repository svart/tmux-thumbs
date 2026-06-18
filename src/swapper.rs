extern crate clap;

use self::clap::{App, Arg};
use clap::crate_version;
use regex::Regex;
use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

trait Executor {
  fn execute(&mut self, args: Vec<String>) -> String;
}

struct RealShell {}

impl RealShell {
  fn new() -> RealShell {
    RealShell {}
  }
}

impl Executor for RealShell {
  fn execute(&mut self, args: Vec<String>) -> String {
    let execution = Command::new(args[0].as_str())
      .args(&args[1..])
      .output()
      .expect("Couldn't run it");

    let output: String = String::from_utf8_lossy(&execution.stdout).into();

    output.trim_end().to_string()
  }
}

const TMP_FILE: &str = "/tmp/thumbs-last";

#[allow(dead_code)]
fn dbg(msg: &str) {
  let mut file = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open("/tmp/thumbs.log")
    .expect("Unable to open log file");

  writeln!(&mut file, "{}", msg).expect("Unable to write log file");
}

pub struct Swapper<'a> {
  executor: &'a mut dyn Executor,
  dir: String,
  command: String,
  upcase_command: String,
  multi_command: String,
  osc52: bool,
  active_pane_id: Option<String>,
  active_pane_height: Option<i32>,
  active_pane_scroll_position: Option<i32>,
  active_pane_zoomed: Option<bool>,
  thumbs_pane_id: Option<String>,
  content: Option<String>,
  signal: String,
  capture_signal: String,
  start_signal: String,
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
    let signal_id = format!("{}-{}", since_the_epoch.as_secs(), since_the_epoch.subsec_nanos());
    let signal = format!("thumbs-finished-{}", signal_id);
    let capture_signal = format!("thumbs-captured-{}", signal_id);
    let start_signal = format!("thumbs-start-{}", signal_id);

    Swapper {
      executor,
      dir,
      command,
      upcase_command,
      multi_command,
      osc52,
      active_pane_id: None,
      active_pane_height: None,
      active_pane_scroll_position: None,
      active_pane_zoomed: None,
      thumbs_pane_id: None,
      content: None,
      signal,
      capture_signal,
      start_signal,
    }
  }

  pub fn capture_active_pane(&mut self) {
    let active_command = [
      "tmux",
      "list-panes",
      "-F",
      "#{pane_id}:#{?pane_in_mode,1,0}:#{pane_height}:#{scroll_position}:#{window_zoomed_flag}:#{?pane_active,active,nope}",
    ];

    let output = self
      .executor
      .execute(active_command.iter().map(|arg| arg.to_string()).collect());

    let lines: Vec<&str> = output.split('\n').collect();
    let chunks: Vec<Vec<&str>> = lines.into_iter().map(|line| line.split(':').collect()).collect();

    let active_pane = chunks
      .iter()
      .find(|&chunks| *chunks.get(5).unwrap() == "active")
      .expect("Unable to find active pane");

    let pane_id = active_pane.first().unwrap();

    self.active_pane_id = Some(pane_id.to_string());

    let pane_height = active_pane
      .get(2)
      .unwrap()
      .parse()
      .expect("Unable to retrieve pane height");

    self.active_pane_height = Some(pane_height);

    if *active_pane.get(1).unwrap() == "1" {
      let pane_scroll_position = active_pane
        .get(3)
        .unwrap()
        .parse()
        .expect("Unable to retrieve pane scroll");

      self.active_pane_scroll_position = Some(pane_scroll_position);
    }

    let zoomed_pane = *active_pane.get(4).expect("Unable to retrieve zoom pane property") == "1";

    self.active_pane_zoomed = Some(zoomed_pane);
  }

  pub fn execute_thumbs(&mut self) {
    let options_command = ["tmux", "show", "-g"];
    let params: Vec<String> = options_command.iter().map(|arg| arg.to_string()).collect();
    let options = self.executor.execute(params);
    let lines: Vec<&str> = options.split('\n').collect();

    let pattern = Regex::new(r#"^@thumbs-([\w\-0-9]+)\s+"?([^"]+)"?$"#).unwrap();

    let args = lines
      .iter()
      .flat_map(|line| {
        if let Some(captures) = pattern.captures(line) {
          let name = captures.get(1).unwrap().as_str();
          let value = captures.get(2).unwrap().as_str();

          let boolean_params = ["reverse", "unique", "contrast"];

          if boolean_params.contains(&name) {
            return vec![format!("--{}", name)];
          }

          let string_params = [
            "alphabet",
            "position",
            "fg-color",
            "bg-color",
            "hint-bg-color",
            "hint-fg-color",
            "select-fg-color",
            "select-bg-color",
            "multi-fg-color",
            "multi-bg-color",
          ];

          if string_params.contains(&name) {
            return vec![format!("--{}", name), format!("'{}'", value)];
          }

          if name.starts_with("regexp") {
            return vec!["--regexp".to_string(), format!("'{}'", value.replace("\\\\", "\\"))];
          }

          vec![]
        } else {
          vec![]
        }
      })
      .collect::<Vec<String>>();

    let active_pane_id = self.active_pane_id.as_mut().unwrap().clone();

    let scroll_params =
      if let (Some(pane_height), Some(scroll_position)) = (self.active_pane_height, self.active_pane_scroll_position) {
        format!(" -S {} -E {}", -scroll_position, pane_height - scroll_position - 1)
      } else {
        "".to_string()
      };

    let active_pane_zoomed = *self.active_pane_zoomed.as_mut().unwrap();
    let zoom_command = if active_pane_zoomed {
      format!("tmux resize-pane -t {} -Z;", active_pane_id)
    } else {
      "".to_string()
    };

    // Capture before swapping panes; once a split pane is moved into the hidden
    // full-window slot, tmux can resize/reflow it and the old height can truncate matches.
    let capture_command = format!(
      "tmux capture-pane -J -t {active_pane_id} -p{scroll_params} | tail -n {height}",
      active_pane_id = active_pane_id,
      scroll_params = scroll_params,
      height = self.active_pane_height.unwrap_or(i32::MAX)
    );

    let pane_command = format!(
        "capture=\"$({capture_command})\"; tmux wait-for -S {capture_signal}; tmux wait-for {start_signal}; printf '%s\\n' \"$capture\" | {dir}/target/release/thumbs -f '%U:%H' -t {tmp} {args}; tmux swap-pane -t {active_pane_id}; {zoom_command} tmux wait-for -S {signal}",
        capture_command = capture_command,
        active_pane_id = active_pane_id,
        dir = self.dir,
        tmp = TMP_FILE,
        args = args.join(" "),
        zoom_command = zoom_command,
        signal = self.signal,
        capture_signal = self.capture_signal,
        start_signal = self.start_signal
    );

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

    self.thumbs_pane_id = Some(self.executor.execute(params));
    self.wait_capture();
  }

  pub fn swap_panes(&mut self) {
    let active_pane_id = self.active_pane_id.as_mut().unwrap().clone();
    let thumbs_pane_id = self.thumbs_pane_id.as_mut().unwrap().clone();

    let swap_command = [
      "tmux",
      "swap-pane",
      "-d",
      "-s",
      active_pane_id.as_str(),
      "-t",
      thumbs_pane_id.as_str(),
    ];

    let params = swap_command
      .iter()
      .filter(|&s| !s.is_empty())
      .map(|arg| arg.to_string())
      .collect();

    self.executor.execute(params);
  }

  pub fn resize_pane(&mut self) {
    let active_pane_zoomed = *self.active_pane_zoomed.as_mut().unwrap();

    if !active_pane_zoomed {
      return;
    }

    let thumbs_pane_id = self.thumbs_pane_id.as_mut().unwrap().clone();

    let resize_command = ["tmux", "resize-pane", "-t", thumbs_pane_id.as_str(), "-Z"];

    let params = resize_command
      .iter()
      .filter(|&s| !s.is_empty())
      .map(|arg| arg.to_string())
      .collect();

    self.executor.execute(params);
  }

  pub fn wait_capture(&mut self) {
    let wait_command = ["tmux", "wait-for", self.capture_signal.as_str()];
    let params = wait_command.iter().map(|arg| arg.to_string()).collect();

    self.executor.execute(params);
  }

  pub fn start_thumbs(&mut self) {
    let start_command = ["tmux", "wait-for", "-S", self.start_signal.as_str()];
    let params = start_command.iter().map(|arg| arg.to_string()).collect();

    self.executor.execute(params);
  }

  pub fn wait_thumbs(&mut self) {
    let wait_command = ["tmux", "wait-for", self.signal.as_str()];
    let params = wait_command.iter().map(|arg| arg.to_string()).collect();

    self.executor.execute(params);
  }

  pub fn retrieve_content(&mut self) {
    let retrieve_command = ["cat", TMP_FILE];
    let params = retrieve_command.iter().map(|arg| arg.to_string()).collect();

    self.content = Some(self.executor.execute(params));
  }

  pub fn destroy_content(&mut self) {
    let retrieve_command = ["rm", TMP_FILE];
    let params = retrieve_command.iter().map(|arg| arg.to_string()).collect();

    self.executor.execute(params);
  }

  pub fn send_osc52(&mut self) {}

  pub fn execute_command(&mut self) {
    let content = self.content.clone().unwrap();
    let items: Vec<&str> = content.split('\n').collect();

    if items.len() > 1 {
      let text = items
        .iter()
        .map(|item| item.splitn(2, ':').last().unwrap())
        .collect::<Vec<&str>>()
        .join(" ");

      self.execute_final_command(&text, &self.multi_command.clone());

      return;
    }

    // Only one item
    let item: &str = items.first().unwrap();

    let mut splitter = item.splitn(2, ':');

    if let Some(upcase) = splitter.next() {
      if let Some(text) = splitter.next() {
        if self.osc52 {
          let base64_text = base64::encode(text.as_bytes());
          let osc_seq = format!("\x1b]52;0;{}\x07", base64_text);
          let tmux_seq = format!("\x1bPtmux;{}\x1b\\", osc_seq.replace("\x1b", "\x1b\x1b"));

          // FIXME: Review if this comment is still rellevant
          //
          // When the user selects a match:
          // 1. The `rustbox` object created in the `viewbox` above is dropped.
          // 2. During its `drop`, the `rustbox` object sends a CSI 1049 escape
          //    sequence to tmux.
          // 3. This escape sequence causes the `window_pane_alternate_off` function
          //    in tmux to be called.
          // 4. In `window_pane_alternate_off`, tmux sets the needs-redraw flag in the
          //    pane.
          // 5. If we print the OSC copy escape sequence before the redraw is completed,
          //    tmux will *not* send the sequence to the host terminal. See the following
          //    call chain in tmux: `input_dcs_dispatch` -> `screen_write_rawstring`
          //    -> `tty_write` -> `tty_client_ready`. In this case, `tty_client_ready`
          //    will return false, thus preventing the escape sequence from being sent.
          //
          // Therefore, for now we wait a little bit here for the redraw to finish.
          std::thread::sleep(std::time::Duration::from_millis(100));

          std::io::stdout().write_all(tmux_seq.as_bytes()).unwrap();
          std::io::stdout().flush().unwrap();
        }

        let execute_command = if upcase.trim_end() == "true" {
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
        // kinds of harmful escaping issues that the user cannot reasonable avoid.
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
        self.execute_final_command(text.trim_end(), &execute_command);
      }
    }
  }

  pub fn execute_final_command(&mut self, text: &str, execute_command: &str) {
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

    self.executor.execute(params);
  }
}

fn app_args<'a>() -> clap::ArgMatches<'a> {
  App::new("tmux-thumbs")
    .version(crate_version!())
    .about("A lightning fast version of tmux-fingers, copy/pasting tmux like vimium/vimperator")
    .arg(
      Arg::with_name("dir")
        .help("Directory where to execute thumbs")
        .long("dir")
        .default_value(""),
    )
    .arg(
      Arg::with_name("command")
        .help("Command to execute after choose a hint")
        .long("command")
        .default_value("tmux set-buffer -- \"{}\" && tmux display-message \"Copied {}\""),
    )
    .arg(
      Arg::with_name("upcase_command")
        .help("Command to execute after choose a hint, in upcase")
        .long("upcase-command")
        .default_value("tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Copied {}\""),
    )
    .arg(
      Arg::with_name("multi_command")
        .help("Command to execute after choose multiple hints")
        .long("multi-command")
        .default_value("tmux set-buffer -- \"{}\" && tmux paste-buffer && tmux display-message \"Multi copied {}\""),
    )
    .arg(
      Arg::with_name("osc52")
        .help("Print OSC52 copy escape sequence in addition to running the pick command")
        .long("osc52")
        .short("o"),
    )
    .get_matches()
}

fn main() -> std::io::Result<()> {
  let args = app_args();
  let dir = args.value_of("dir").unwrap();
  let command = args.value_of("command").unwrap();
  let upcase_command = args.value_of("upcase_command").unwrap();
  let multi_command = args.value_of("multi_command").unwrap();
  let osc52 = args.is_present("osc52");

  if dir.is_empty() {
    panic!("Invalid tmux-thumbs execution. Are you trying to execute tmux-thumbs directly?")
  }

  let mut executor = RealShell::new();
  let mut swapper = Swapper::new(
    &mut executor,
    dir.to_string(),
    command.to_string(),
    upcase_command.to_string(),
    multi_command.to_string(),
    osc52,
  );

  swapper.capture_active_pane();
  swapper.execute_thumbs();
  swapper.swap_panes();
  swapper.resize_pane();
  swapper.start_thumbs();
  swapper.wait_thumbs();
  swapper.retrieve_content();
  swapper.destroy_content();
  swapper.execute_command();

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  struct TestShell {
    outputs: Vec<String>,
    executed: Option<Vec<String>>,
    executions: Vec<Vec<String>>,
  }

  impl TestShell {
    fn new(outputs: Vec<String>) -> TestShell {
      TestShell {
        executed: None,
        outputs,
        executions: vec![],
      }
    }

    fn last_executed(&self) -> Option<Vec<String>> {
      self.executed.clone()
    }
  }

  impl Executor for TestShell {
    fn execute(&mut self, args: Vec<String>) -> String {
      self.executed = Some(args);
      self.executions.push(self.executed.clone().unwrap());
      self.outputs.pop().unwrap()
    }
  }

  #[test]
  fn retrieve_active_pane() {
    let last_command_outputs = vec!["%97:100:24:1:0:active\n%106:100:24:1:0:nope\n%107:100:24:1:0:nope\n".to_string()];
    let mut executor = TestShell::new(last_command_outputs);
    let mut swapper = Swapper::new(
      &mut executor,
      "".to_string(),
      "".to_string(),
      "".to_string(),
      "".to_string(),
      false,
    );

    swapper.capture_active_pane();

    assert_eq!(swapper.active_pane_id.unwrap(), "%97");
  }

  #[test]
  fn swap_panes() {
    let last_command_outputs = vec![
      "".to_string(),
      "".to_string(),
      "%100".to_string(),
      "".to_string(),
      "%106:100:24:1:0:nope\n%98:100:24:1:0:active\n%107:100:24:1:0:nope\n".to_string(),
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

    swapper.capture_active_pane();
    swapper.execute_thumbs();
    swapper.swap_panes();

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
      "%106:0:12:0:0:nope\n%98:0:8:0:0:active\n%107:0:12:0:0:nope\n".to_string(),
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

    swapper.capture_active_pane();
    swapper.execute_thumbs();
    swapper.swap_panes();
    swapper.start_thumbs();

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
      .position(|command| command.as_slice() == ["tmux", "swap-pane", "-d", "-s", "%98", "-t", "%100"])
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

    let capture_index = pane_command
      .find("capture=\"$(tmux capture-pane -J -t %98 -p | tail -n 8)\"")
      .unwrap();
    let capture_signal_index = pane_command.find("tmux wait-for -S thumbs-captured-").unwrap();
    let start_wait_index = pane_command.find("tmux wait-for thumbs-start-").unwrap();
    let thumbs_index = pane_command.find("/target/release/thumbs").unwrap();

    assert!(capture_index < capture_signal_index);
    assert!(capture_signal_index < start_wait_index);
    assert!(start_wait_index < thumbs_index);
  }

  #[test]
  fn quoted_execution() {
    let last_command_outputs = vec!["Blah blah blah, the ignored user script output".to_string()];
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
    swapper.execute_command();

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
  }
}
