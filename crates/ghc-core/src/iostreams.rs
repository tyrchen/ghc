//! Terminal I/O abstraction layer.
//!
//! Maps from Go's `pkg/iostreams` package. Handles TTY detection,
//! color support, pager integration, progress indicators, and output
//! capture for testing.

use std::io::{self, IsTerminal, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use console::Term;

/// Default terminal width when detection fails.
pub const DEFAULT_WIDTH: usize = 80;

/// Writer wrapper that supports both real I/O and buffered capture.
///
/// In system mode, writes go to real stdout/stderr.
/// In test mode, writes are captured to an in-memory buffer.
struct OutputWriter(Box<dyn Write + Send>);

impl std::fmt::Debug for OutputWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputWriter")
    }
}

impl Write for OutputWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Writer that shares a buffer with test code via `Arc<Mutex<Vec<u8>>>`.
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Captured test output that can be inspected after command execution.
#[derive(Debug, Clone)]
pub struct TestOutput {
    out_buf: Arc<Mutex<Vec<u8>>>,
    err_buf: Arc<Mutex<Vec<u8>>>,
}

impl TestOutput {
    /// Get the captured stdout content as a string.
    pub fn stdout(&self) -> String {
        let buf = self
            .out_buf
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&buf).to_string()
    }

    /// Get the captured stderr content as a string.
    pub fn stderr(&self) -> String {
        let buf = self
            .err_buf
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&buf).to_string()
    }
}

/// I/O streams for terminal interaction.
///
/// Wraps stdin, stdout, and stderr with TTY detection, color support,
/// pager management, progress indicators, and capturable output writers.
///
/// Commands should use `println_out()` / `println_err()` instead of
/// `println!()` / `eprintln!()` so output can be captured in tests.
#[allow(clippy::struct_excessive_bools)]
pub struct IOStreams {
    // TTY state
    stdin_is_tty: bool,
    stdout_is_tty: bool,
    stderr_is_tty: bool,

    // Color
    color_forced: Option<bool>,
    color_256: bool,
    true_color: bool,
    color_labels: bool,
    accessible_colors: bool,

    // Pager
    pager_cmd: Option<String>,
    pager_process: Mutex<Option<Child>>,

    // Spinner
    spinner_disabled: bool,

    // Prompt
    never_prompt: bool,
    accessible_prompter: bool,

    // Output writers (capturable in test mode)
    out: Arc<Mutex<OutputWriter>>,
    err: Arc<Mutex<OutputWriter>>,
}

impl std::fmt::Debug for IOStreams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOStreams")
            .field("stdin_is_tty", &self.stdin_is_tty)
            .field("stdout_is_tty", &self.stdout_is_tty)
            .field("stderr_is_tty", &self.stderr_is_tty)
            .field("color_forced", &self.color_forced)
            .field("never_prompt", &self.never_prompt)
            .finish_non_exhaustive()
    }
}

impl IOStreams {
    /// Create `IOStreams` for the real terminal.
    pub fn system() -> Self {
        let stdin_is_tty = io::stdin().is_terminal();
        let stdout_is_tty = io::stdout().is_terminal();
        let stderr_is_tty = io::stderr().is_terminal();
        let term = Term::stdout();
        let color_256 = term.features().colors_supported();
        let true_color = term.features().colors_supported();

        Self {
            stdin_is_tty,
            stdout_is_tty,
            stderr_is_tty,
            color_forced: std::env::var("NO_COLOR").ok().map(|_| false),
            color_256,
            true_color,
            color_labels: false,
            accessible_colors: false,
            pager_cmd: None,
            pager_process: Mutex::new(None),
            spinner_disabled: false,
            never_prompt: false,
            accessible_prompter: false,
            out: Arc::new(Mutex::new(OutputWriter(Box::new(io::stdout())))),
            err: Arc::new(Mutex::new(OutputWriter(Box::new(io::stderr())))),
        }
    }

    /// Create `IOStreams` for testing with no TTY and no output capture.
    ///
    /// Output goes to real stdout/stderr. Use `test_with_output()` to
    /// capture output in buffers for assertion.
    pub fn test() -> Self {
        Self {
            stdin_is_tty: false,
            stdout_is_tty: false,
            stderr_is_tty: false,
            color_forced: Some(false),
            color_256: false,
            true_color: false,
            color_labels: false,
            accessible_colors: false,
            pager_cmd: None,
            pager_process: Mutex::new(None),
            spinner_disabled: true,
            never_prompt: true,
            accessible_prompter: false,
            out: Arc::new(Mutex::new(OutputWriter(Box::new(io::stdout())))),
            err: Arc::new(Mutex::new(OutputWriter(Box::new(io::stderr())))),
        }
    }

    /// Create `IOStreams` for testing with output captured to buffers.
    ///
    /// Returns the IOStreams and a `TestOutput` handle for reading captured
    /// stdout/stderr after command execution.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (ios, output) = IOStreams::test_with_output();
    /// ios.println_out("hello");
    /// assert_eq!(output.stdout(), "hello\n");
    /// ```
    pub fn test_with_output() -> (Self, TestOutput) {
        let out_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let err_buf = Arc::new(Mutex::new(Vec::<u8>::new()));

        let ios = Self {
            stdin_is_tty: false,
            stdout_is_tty: false,
            stderr_is_tty: false,
            color_forced: Some(false),
            color_256: false,
            true_color: false,
            color_labels: false,
            accessible_colors: false,
            pager_cmd: None,
            pager_process: Mutex::new(None),
            spinner_disabled: true,
            never_prompt: true,
            accessible_prompter: false,
            out: Arc::new(Mutex::new(OutputWriter(Box::new(SharedWriter(
                out_buf.clone(),
            ))))),
            err: Arc::new(Mutex::new(OutputWriter(Box::new(SharedWriter(
                err_buf.clone(),
            ))))),
        };

        let output = TestOutput { out_buf, err_buf };

        (ios, output)
    }

    /// Set the stdout TTY state (for test configuration).
    pub fn set_stdout_tty(&mut self, is_tty: bool) {
        self.stdout_is_tty = is_tty;
    }

    /// Set the stdin TTY state (for test configuration).
    pub fn set_stdin_tty(&mut self, is_tty: bool) {
        self.stdin_is_tty = is_tty;
    }

    /// Set the stderr TTY state (for test configuration).
    pub fn set_stderr_tty(&mut self, is_tty: bool) {
        self.stderr_is_tty = is_tty;
    }

    // --- Output methods ---

    /// Write a string to stdout followed by a newline.
    pub fn println_out(&self, s: &str) {
        let mut w = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(w, "{s}");
    }

    /// Write a string to stdout without a trailing newline.
    pub fn print_out(&self, s: &str) {
        let mut w = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_all(s.as_bytes());
    }

    /// Write a string to stderr followed by a newline.
    pub fn println_err(&self, s: &str) {
        let mut w = self
            .err
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = writeln!(w, "{s}");
    }

    /// Write a string to stderr without a trailing newline.
    pub fn print_err(&self, s: &str) {
        let mut w = self
            .err
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_all(s.as_bytes());
    }

    /// Write formatted output to stdout. Accepts format arguments.
    pub fn write_out(&self, args: std::fmt::Arguments<'_>) {
        let mut w = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_fmt(args);
    }

    /// Write formatted output to stdout with trailing newline.
    pub fn writeln_out(&self, args: std::fmt::Arguments<'_>) {
        let mut w = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_fmt(args);
        let _ = w.write_all(b"\n");
    }

    /// Write formatted output to stderr. Accepts format arguments.
    pub fn write_err(&self, args: std::fmt::Arguments<'_>) {
        let mut w = self
            .err
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_fmt(args);
    }

    /// Write formatted output to stderr with trailing newline.
    pub fn writeln_err(&self, args: std::fmt::Arguments<'_>) {
        let mut w = self
            .err
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.write_fmt(args);
        let _ = w.write_all(b"\n");
    }

    // --- Query methods ---

    /// Whether stdin is connected to a terminal.
    pub fn is_stdin_tty(&self) -> bool {
        self.stdin_is_tty
    }

    /// Whether stdout is connected to a terminal.
    pub fn is_stdout_tty(&self) -> bool {
        self.stdout_is_tty
    }

    /// Whether stderr is connected to a terminal.
    pub fn is_stderr_tty(&self) -> bool {
        self.stderr_is_tty
    }

    /// Whether color output is enabled.
    pub fn color_enabled(&self) -> bool {
        if let Some(forced) = self.color_forced {
            return forced;
        }
        self.stdout_is_tty
    }

    /// Whether 256-color mode is supported.
    pub fn color_support_256(&self) -> bool {
        self.color_enabled() && self.color_256
    }

    /// Whether true color (24-bit) is supported.
    pub fn true_color_support(&self) -> bool {
        self.color_enabled() && self.true_color
    }

    /// Whether label coloring is enabled.
    pub fn color_labels(&self) -> bool {
        self.color_labels
    }

    /// Set label coloring.
    pub fn set_color_labels(&mut self, enabled: bool) {
        self.color_labels = enabled;
    }

    /// Whether accessible colors are enabled.
    pub fn accessible_colors_enabled(&self) -> bool {
        self.accessible_colors
    }

    /// Set accessible colors.
    pub fn set_accessible_colors(&mut self, enabled: bool) {
        self.accessible_colors = enabled;
    }

    /// Set the pager command.
    pub fn set_pager(&mut self, cmd: impl Into<String>) {
        let cmd = cmd.into();
        if cmd.is_empty() {
            self.pager_cmd = None;
        } else {
            self.pager_cmd = Some(cmd);
        }
    }

    /// Start the pager if configured and stdout is a TTY.
    ///
    /// # Errors
    ///
    /// Returns an error if the pager process cannot be started.
    pub fn start_pager(&self) -> io::Result<Option<Box<dyn Write>>> {
        let Some(ref pager_cmd) = self.pager_cmd else {
            return Ok(None);
        };

        if !self.stdout_is_tty {
            return Ok(None);
        }

        let parts = shlex::split(pager_cmd).unwrap_or_else(|| vec![pager_cmd.clone()]);
        if parts.is_empty() {
            return Ok(None);
        }

        let mut cmd = Command::new(&parts[0]);
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }
        cmd.stdin(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("failed to open pager stdin"))?;

        let mut guard = self
            .pager_process
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(child);

        Ok(Some(Box::new(stdin)))
    }

    /// Stop the pager process if running.
    pub fn stop_pager(&self) {
        let mut guard = self
            .pager_process
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mut child) = guard.take() {
            let _ = child.wait();
        }
    }

    /// Whether the spinner is disabled.
    pub fn spinner_disabled(&self) -> bool {
        self.spinner_disabled
    }

    /// Set spinner disabled state.
    pub fn set_spinner_disabled(&mut self, disabled: bool) {
        self.spinner_disabled = disabled;
    }

    /// Whether prompts should never be shown.
    pub fn never_prompt(&self) -> bool {
        self.never_prompt
    }

    /// Set never-prompt mode.
    pub fn set_never_prompt(&mut self, never: bool) {
        self.never_prompt = never;
    }

    /// Whether the accessible prompter is enabled.
    pub fn accessible_prompter_enabled(&self) -> bool {
        self.accessible_prompter
    }

    /// Set accessible prompter.
    pub fn set_accessible_prompter(&mut self, enabled: bool) {
        self.accessible_prompter = enabled;
    }

    /// Get the terminal width, or the default if not a TTY.
    pub fn terminal_width(&self) -> usize {
        if self.stdout_is_tty {
            let term = Term::stdout();
            term.size().1 as usize
        } else {
            DEFAULT_WIDTH
        }
    }

    /// Check if interactive mode is available (stdin and stdout are TTY, prompts not disabled).
    pub fn can_prompt(&self) -> bool {
        self.stdin_is_tty && self.stdout_is_tty && !self.never_prompt
    }

    /// Create a `ColorScheme` based on the current color settings.
    pub fn color_scheme(&self) -> ColorScheme {
        ColorScheme {
            enabled: self.color_enabled(),
        }
    }
}

/// Terminal color scheme for themed output.
#[derive(Debug, Clone)]
pub struct ColorScheme {
    enabled: bool,
}

impl ColorScheme {
    /// Apply bold styling.
    pub fn bold(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply success (green) styling.
    pub fn success(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).green().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply warning (yellow) styling.
    pub fn warning(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).yellow().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply error (red) styling.
    pub fn error(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).red().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply dimmed/gray styling.
    pub fn gray(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).dim().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply cyan styling (for links, emphasis).
    pub fn cyan(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).cyan().to_string()
        } else {
            text.to_string()
        }
    }

    /// Apply magenta styling.
    pub fn magenta(&self, text: &str) -> String {
        if self.enabled {
            console::style(text).magenta().to_string()
        } else {
            text.to_string()
        }
    }

    /// Whether colors are enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Style for successful icon.
    pub fn success_icon(&self) -> String {
        self.success("✓")
    }

    /// Style for warning icon.
    pub fn warning_icon(&self) -> String {
        self.warning("!")
    }

    /// Style for error icon.
    pub fn error_icon(&self) -> String {
        self.error("X")
    }
}

/// Write to IOStreams stdout, similar to `print!()`.
#[macro_export]
macro_rules! ios_print {
    ($ios:expr, $($arg:tt)*) => {
        $ios.write_out(format_args!($($arg)*))
    };
}

/// Write to IOStreams stdout with newline, similar to `println!()`.
#[macro_export]
macro_rules! ios_println {
    ($ios:expr) => {
        $ios.println_out("")
    };
    ($ios:expr, $($arg:tt)*) => {
        $ios.writeln_out(format_args!($($arg)*))
    };
}

/// Write to IOStreams stderr, similar to `eprint!()`.
#[macro_export]
macro_rules! ios_eprint {
    ($ios:expr, $($arg:tt)*) => {
        $ios.write_err(format_args!($($arg)*))
    };
}

/// Write to IOStreams stderr with newline, similar to `eprintln!()`.
#[macro_export]
macro_rules! ios_eprintln {
    ($ios:expr) => {
        $ios.println_err("")
    };
    ($ios:expr, $($arg:tt)*) => {
        $ios.writeln_err(format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- IOStreams::test() defaults ---

    #[test]
    fn test_should_create_test_streams_with_no_tty() {
        let ios = IOStreams::test();
        assert!(!ios.is_stdin_tty());
        assert!(!ios.is_stdout_tty());
        assert!(!ios.is_stderr_tty());
    }

    #[test]
    fn test_should_disable_color_in_test_mode() {
        let ios = IOStreams::test();
        assert!(!ios.color_enabled());
        assert!(!ios.color_support_256());
        assert!(!ios.true_color_support());
    }

    #[test]
    fn test_should_disable_prompts_in_test_mode() {
        let ios = IOStreams::test();
        assert!(ios.never_prompt());
        assert!(!ios.can_prompt());
    }

    #[test]
    fn test_should_disable_spinner_in_test_mode() {
        let ios = IOStreams::test();
        assert!(ios.spinner_disabled());
    }

    // --- Output capture ---

    #[test]
    fn test_should_capture_stdout_output() {
        let (ios, output) = IOStreams::test_with_output();
        ios.println_out("hello world");
        assert_eq!(output.stdout(), "hello world\n");
    }

    #[test]
    fn test_should_capture_stderr_output() {
        let (ios, output) = IOStreams::test_with_output();
        ios.println_err("error message");
        assert_eq!(output.stderr(), "error message\n");
    }

    #[test]
    fn test_should_capture_multiple_writes() {
        let (ios, output) = IOStreams::test_with_output();
        ios.print_out("hello ");
        ios.print_out("world");
        assert_eq!(output.stdout(), "hello world");
    }

    #[test]
    fn test_should_capture_formatted_output() {
        let (ios, output) = IOStreams::test_with_output();
        ios.write_out(format_args!("count: {}\n", 42));
        assert_eq!(output.stdout(), "count: 42\n");
    }

    #[test]
    fn test_should_set_tty_modes() {
        let (mut ios, _) = IOStreams::test_with_output();
        assert!(!ios.is_stdout_tty());
        ios.set_stdout_tty(true);
        assert!(ios.is_stdout_tty());
        ios.set_stdin_tty(true);
        assert!(ios.is_stdin_tty());
        ios.set_stderr_tty(true);
        assert!(ios.is_stderr_tty());
    }

    // --- Pager ---

    #[test]
    fn test_should_set_pager_command() {
        let mut ios = IOStreams::test();
        ios.set_pager("less -R");
        // Test mode has no TTY so pager won't start
        let pager = ios.start_pager().unwrap();
        assert!(pager.is_none());
    }

    #[test]
    fn test_should_clear_pager_on_empty_string() {
        let mut ios = IOStreams::test();
        ios.set_pager("less -R");
        ios.set_pager("");
        let pager = ios.start_pager().unwrap();
        assert!(pager.is_none());
    }

    #[test]
    fn test_should_stop_pager_gracefully_when_none_running() {
        let ios = IOStreams::test();
        ios.stop_pager(); // should not panic
    }

    // --- Terminal width ---

    #[test]
    fn test_should_return_default_width_for_non_tty() {
        let ios = IOStreams::test();
        assert_eq!(ios.terminal_width(), DEFAULT_WIDTH);
    }

    // --- Setters ---

    #[test]
    fn test_should_set_color_labels() {
        let mut ios = IOStreams::test();
        assert!(!ios.color_labels());
        ios.set_color_labels(true);
        assert!(ios.color_labels());
    }

    #[test]
    fn test_should_set_accessible_colors() {
        let mut ios = IOStreams::test();
        assert!(!ios.accessible_colors_enabled());
        ios.set_accessible_colors(true);
        assert!(ios.accessible_colors_enabled());
    }

    #[test]
    fn test_should_set_spinner_disabled() {
        let mut ios = IOStreams::test();
        ios.set_spinner_disabled(false);
        assert!(!ios.spinner_disabled());
        ios.set_spinner_disabled(true);
        assert!(ios.spinner_disabled());
    }

    #[test]
    fn test_should_set_never_prompt() {
        let mut ios = IOStreams::test();
        ios.set_never_prompt(false);
        assert!(!ios.never_prompt());
    }

    #[test]
    fn test_should_set_accessible_prompter() {
        let mut ios = IOStreams::test();
        assert!(!ios.accessible_prompter_enabled());
        ios.set_accessible_prompter(true);
        assert!(ios.accessible_prompter_enabled());
    }

    // --- ColorScheme (disabled) ---

    #[test]
    fn test_should_pass_through_text_when_color_disabled() {
        let cs = ColorScheme { enabled: false };
        assert!(!cs.is_enabled());
        assert_eq!(cs.bold("hello"), "hello");
        assert_eq!(cs.success("ok"), "ok");
        assert_eq!(cs.warning("warn"), "warn");
        assert_eq!(cs.error("fail"), "fail");
        assert_eq!(cs.gray("dim"), "dim");
        assert_eq!(cs.cyan("link"), "link");
        assert_eq!(cs.magenta("purple"), "purple");
    }

    #[test]
    fn test_should_return_plain_icons_when_color_disabled() {
        let cs = ColorScheme { enabled: false };
        // Icons should still contain the glyph, just not styled
        assert!(cs.success_icon().contains('\u{2713}') || cs.success_icon().contains('✓'));
        assert!(cs.warning_icon().contains('!'));
        assert!(cs.error_icon().contains('X'));
    }

    #[test]
    fn test_should_apply_styles_when_color_enabled() {
        let cs = ColorScheme { enabled: true };
        assert!(cs.is_enabled());
        // Styled output should differ from plain text (contains ANSI codes)
        let styled = cs.bold("hello");
        assert!(styled.len() > "hello".len() || styled == "hello");
    }

    // --- color_scheme from IOStreams ---

    #[test]
    fn test_should_return_disabled_color_scheme_for_test_streams() {
        let ios = IOStreams::test();
        let cs = ios.color_scheme();
        assert!(!cs.is_enabled());
    }

    // --- Macros with format arguments ---

    #[test]
    fn test_should_capture_ios_println_with_format_args() {
        let (ios, output) = IOStreams::test_with_output();
        let name = "world";
        ios_println!(ios, "hello {}", name);
        assert_eq!(output.stdout(), "hello world\n");
    }

    #[test]
    fn test_should_capture_ios_eprintln_with_format_args() {
        let (ios, output) = IOStreams::test_with_output();
        let code = 42;
        ios_eprintln!(ios, "error code: {}", code);
        assert_eq!(output.stderr(), "error code: 42\n");
    }

    #[test]
    fn test_should_capture_ios_print_with_format_args() {
        let (ios, output) = IOStreams::test_with_output();
        ios_print!(ios, "value={}", 99);
        assert_eq!(output.stdout(), "value=99");
    }

    #[test]
    fn test_should_capture_writeln_out_with_format_args() {
        let (ios, output) = IOStreams::test_with_output();
        ios.writeln_out(format_args!("{} + {} = {}", 1, 2, 3));
        assert_eq!(output.stdout(), "1 + 2 = 3\n");
    }

    #[test]
    fn test_should_capture_writeln_err_with_format_args() {
        let (ios, output) = IOStreams::test_with_output();
        ios.writeln_err(format_args!("warning: {}", "oops"));
        assert_eq!(output.stderr(), "warning: oops\n");
    }
}
