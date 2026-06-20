use crate::tmux_options::shell_quote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneScript<'a> {
    pub(crate) dir: &'a str,
    pub(crate) result_path: &'a str,
    pub(crate) picker_args: &'a [String],
    pub(crate) active_pane_id: &'a str,
    pub(crate) active_pane_height: i32,
    pub(crate) active_pane_scroll_position: Option<i32>,
    pub(crate) active_pane_zoomed: bool,
    pub(crate) finished_signal: &'a str,
    pub(crate) captured_signal: &'a str,
    pub(crate) start_signal: &'a str,
}

impl PaneScript<'_> {
    pub(crate) fn render(&self) -> String {
        let active_pane_id = shell_quote(self.active_pane_id);
        let scroll_params = if let Some(scroll_position) = self.active_pane_scroll_position {
            format!(
                " -S {} -E {}",
                -scroll_position,
                self.active_pane_height - scroll_position - 1
            )
        } else {
            "".to_string()
        };

        // Capture before swapping panes; once a split pane is moved into the hidden
        // full-window slot, tmux can resize/reflow it and the old height can truncate matches.
        let capture_command = format!(
            "tmux capture-pane -J -t {active_pane_id} -p{scroll_params} | tail -n {height}",
            active_pane_id = active_pane_id,
            scroll_params = scroll_params,
            height = self.active_pane_height
        );

        let picker_path = shell_quote(&format!("{}/target/release/thumbs", self.dir));
        let picker_command = format!(
            "printf '%s\\n' \"$capture\" | {} -f {} -t {} {}",
            picker_path,
            shell_quote("%U:%H"),
            shell_quote(self.result_path),
            self.picker_args.join(" ")
        );
        let finished_signal_command =
            format!("tmux wait-for -S {}", shell_quote(self.finished_signal));
        let mut pane_script = vec![
            format!("trap {} EXIT", shell_quote(&finished_signal_command)),
            format!("capture=\"$({})\"", capture_command),
            format!("tmux wait-for -S {}", shell_quote(self.captured_signal)),
            format!("tmux wait-for {}", shell_quote(self.start_signal)),
            picker_command,
            format!("tmux swap-pane -t {}", active_pane_id),
        ];

        if self.active_pane_zoomed {
            pane_script.push(format!("tmux resize-pane -t {} -Z", active_pane_id));
        }

        pane_script.push(finished_signal_command);
        pane_script.join("; ")
    }
}
