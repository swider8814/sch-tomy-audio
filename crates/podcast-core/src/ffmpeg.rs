//! Locating and running ffmpeg/ffprobe.
//!
//! Safety rules (see AGENTS.md):
//! * arguments are always passed as a list to `std::process::Command`, never
//!   through a shell;
//! * input files are passed as absolute paths with a `file:` prefix and with
//!   `-protocol_whitelist file`, so a file name can never be parsed as an
//!   option or as a network protocol;
//! * child processes are killed when the operation is cancelled.

use crate::error::{CoreError, ErrorKind, Result};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// Environment variables that override the tool locations (tests, development).
pub const ENV_FFMPEG: &str = "SCHTOMY_FFMPEG";
pub const ENV_FFPROBE: &str = "SCHTOMY_FFPROBE";

/// Maximum amount of stderr kept in memory per process (the tail is kept).
const STDERR_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Tools {
    /// Finds ffmpeg and ffprobe: environment override, then next to the
    /// running executable (Tauri sidecars), then `PATH`.
    pub fn locate() -> Result<Tools> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let ffmpeg = find_tool("ffmpeg", ENV_FFMPEG, exe_dir.as_deref())?;
        let ffprobe = find_tool("ffprobe", ENV_FFPROBE, exe_dir.as_deref())?;
        Ok(Tools { ffmpeg, ffprobe })
    }
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Finds a bundled tool (env override, next to the executable, PATH).
pub fn find_tool_path(name: &str, env: &str) -> Result<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    find_tool(name, env, exe_dir.as_deref())
}

fn find_tool(name: &str, env: &str, exe_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = std::env::var_os(env) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(CoreError::new(
            ErrorKind::ToolMissing,
            format!("{env} points to a missing file: {}", p.display()),
        ));
    }
    if let Some(dir) = exe_dir {
        let candidate = dir.join(exe_name(name));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(exe_name(name));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(CoreError::new(
        ErrorKind::ToolMissing,
        format!("{name} not found next to the executable nor on PATH"),
    ))
}

/// Cancellation flag shared between the UI thread and workers.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(CoreError::cancelled())
        } else {
            Ok(())
        }
    }
}

/// Builds the ffmpeg/ffprobe argument for an input file: absolute path with a
/// `file:` protocol prefix. A relative name such as `-i.mp3` or `http:x.mp3`
/// can therefore never be read as an option or a network URL.
pub fn input_arg(path: &Path) -> OsString {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut s = OsString::from("file:");
    s.push(abs.as_os_str());
    s
}

/// Same as [`input_arg`], for output files.
pub fn output_arg(path: &Path) -> OsString {
    input_arg(path)
}

#[derive(Debug, Default)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// How stdout of the child is consumed.
pub enum StdoutMode<'a> {
    /// Collect raw bytes (PCM decoding, JSON from ffprobe).
    Capture,
    /// Parse `-progress pipe:1` lines and report processed seconds.
    Progress(&'a mut dyn FnMut(f64)),
    /// Hand every stdout line (UTF-8, lossy) to the callback (yt-dlp).
    Lines(&'a mut dyn FnMut(&str)),
}

impl StdoutMode<'_> {
    fn dispatch(&mut self, line: &str) {
        match self {
            StdoutMode::Progress(cb) => {
                if let Some(sec) = crate::parse::parse_progress_line(line) {
                    cb(sec);
                }
            }
            StdoutMode::Lines(cb) => cb(line),
            StdoutMode::Capture => {}
        }
    }
}

/// Runs a tool with the given argument list. Never uses a shell.
pub fn run(
    program: &Path,
    args: &[OsString],
    cancel: &CancelToken,
    mut mode: StdoutMode<'_>,
) -> Result<RunOutput> {
    cancel.check()?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        // yt-dlp is a Python program: make its output UTF-8 on every system
        // (Polish titles on Windows). Harmless for ffmpeg.
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        CoreError::new(
            ErrorKind::ToolMissing,
            format!("cannot start {}: {e}", program.display()),
        )
    })?;

    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_thread = std::thread::spawn(move || read_tail(stderr));

    let stdout = child.stdout.take().expect("stdout piped");
    let (tx, rx) = mpsc::channel::<String>();
    let line_mode = !matches!(mode, StdoutMode::Capture);
    let stdout_thread = std::thread::spawn(move || -> Vec<u8> {
        if line_mode {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf);
                        let _ = tx.send(line.trim_end_matches(['\r', '\n']).to_string());
                    }
                }
            }
            Vec::new()
        } else {
            let mut buf = Vec::new();
            let mut stdout = stdout;
            let _ = stdout.read_to_end(&mut buf);
            buf
        }
    });

    let status = loop {
        if cancel.is_cancelled() {
            kill_tree(&mut child);
            let _ = child.wait();
            // The reader threads end when the last process holding the pipes
            // is gone; never block the caller on them (a stray grandchild
            // must not make "Anuluj" hang).
            drop(stdout_thread);
            drop(stderr_thread);
            return Err(CoreError::cancelled());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => mode.dispatch(&line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // stdout closed: the process is finishing.
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                return Err(CoreError::new(
                    ErrorKind::Processing,
                    format!("waiting for {}: {e}", program.display()),
                ))
            }
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    // Lines printed just before the process exited.
    while let Ok(line) = rx.try_recv() {
        mode.dispatch(&line);
    }
    let stderr = stderr_thread.join().unwrap_or_default();
    if cancel.is_cancelled() {
        return Err(CoreError::cancelled());
    }
    if !status.success() {
        return Err(CoreError::new(
            ErrorKind::Processing,
            format!(
                "{} exited with {status}\n{}",
                display_command(program, args),
                tail_lines(&stderr, 40)
            ),
        ));
    }
    Ok(RunOutput { stdout, stderr })
}

fn read_tail(mut r: impl Read) -> String {
    let mut out: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                if out.len() > STDERR_LIMIT * 2 {
                    let cut = out.len() - STDERR_LIMIT;
                    out.drain(..cut);
                }
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Last `n` lines of a log, for error details.
pub fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Human readable command line for logs (never executed).
pub fn display_command(program: &Path, args: &[OsString]) -> String {
    let mut s = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for a in args {
        s.push(' ');
        let a = a.to_string_lossy();
        if a.contains(' ') || a.contains(';') || a.is_empty() {
            s.push('"');
            s.push_str(&a);
            s.push('"');
        } else {
            s.push_str(&a);
        }
    }
    s
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Unix: every tool runs in its own process group, so cancelling can kill the
/// whole tree (yt-dlp's standalone builds start a child process).
#[cfg(not(windows))]
fn hide_console(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

/// Kills the child and everything it started.
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    {
        // the group id equals the child's pid (process_group(0))
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Small helper to build argument lists.
#[derive(Debug, Default, Clone)]
pub struct Args(pub Vec<OsString>);

impl Args {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Self {
        self.0.push(a.as_ref().to_os_string());
        self
    }
    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in it {
            self.0.push(a.as_ref().to_os_string());
        }
        self
    }
    /// `-protocol_whitelist file -i file:/abs/path`
    pub fn input(self, path: &Path) -> Self {
        self.args(["-protocol_whitelist", "file", "-i"])
            .arg(input_arg(path))
    }
    pub fn into_vec(self) -> Vec<OsString> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_arg_is_absolute_with_file_prefix() {
        let a = input_arg(Path::new("-dziwna nazwa ąę.mp3"));
        let s = a.to_string_lossy();
        assert!(s.starts_with("file:"));
        let rest = &s["file:".len()..];
        assert!(Path::new(rest).is_absolute(), "{rest}");
        assert!(rest.ends_with("-dziwna nazwa ąę.mp3"));
    }

    #[test]
    fn args_input_adds_whitelist() {
        let v = Args::new().input(Path::new("/tmp/a b.mp3")).into_vec();
        assert_eq!(v[0], "-protocol_whitelist");
        assert_eq!(v[1], "file");
        assert_eq!(v[2], "-i");
        assert!(v[3].to_string_lossy().starts_with("file:"));
    }

    #[test]
    fn cancel_token() {
        let t = CancelToken::new();
        assert!(t.check().is_ok());
        let t2 = t.clone();
        t2.cancel();
        assert!(t.check().unwrap_err().is_cancelled());
    }

    #[test]
    fn tail_lines_keeps_last() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(tail_lines("a", 5), "a");
    }
}
