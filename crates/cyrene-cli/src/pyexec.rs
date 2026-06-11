//! Python script execution for the chat REPL.
//!
//! Cyrene can both *write* Python (in its replies) and *run* it. This module is
//! the "run" half: it locates an interpreter, executes inline code or a file in
//! a scratch workspace, and captures stdout/stderr so the result can flow back
//! into the conversation. It also extracts fenced ```python blocks from a model
//! reply so the REPL can offer to execute generated scripts (the Hermes-style
//! "agent writes a script and runs it" loop).
//!
//! Safety: running code is never silent or implicit. Inline `/py` and `/run`
//! are explicit user actions, and auto-running blocks from a reply is gated
//! behind a per-session opt-in (`/autorun`) plus a printed preview.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Result of running a Python snippet or file.
pub struct PyOutcome {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl PyOutcome {
    /// A compact, conversation-friendly rendering of the run, suitable for both
    /// printing to the user and feeding back to the model as a tool result.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::new();
        if !self.stdout.trim().is_empty() {
            out.push_str(self.stdout.trim_end());
            out.push('\n');
        }
        if !self.stderr.trim().is_empty() {
            out.push_str("[stderr] ");
            out.push_str(self.stderr.trim_end());
            out.push('\n');
        }
        match self.status {
            Some(0) => out.push_str("[exit 0]"),
            Some(c) => out.push_str(&format!("[exit {c}]")),
            None => out.push_str("[terminated]"),
        }
        out
    }
}

/// Finds a usable Python interpreter, preferring `python3`.
#[must_use]
pub fn interpreter() -> Option<&'static str> {
    fn works(cand: &str) -> bool {
        Command::new(cand)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    ["python3", "python"].into_iter().find(|c| works(c))
}

/// The per-user scratch directory where inline snippets and generated scripts
/// run: `~/.cyrene/scripts`. Created on demand.
#[must_use]
pub fn scripts_dir() -> PathBuf {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let dir = base.join("scripts");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Sanitizes a user-supplied script name into a safe file stem: lowercase
/// alphanumerics, `-`, and `_` only. Returns `None` if nothing usable remains
/// (so we never write outside the scripts dir or create odd filenames).
#[must_use]
pub fn sanitize_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Saves Python source as a durable, re-runnable script
/// `~/.cyrene/scripts/<name>.py` and returns its path. This is how a script
/// Cyrene wrote in chat becomes a named integration that cron can schedule.
pub fn save_script(name: &str, code: &str) -> Result<PathBuf, String> {
    let stem = sanitize_name(name).ok_or_else(|| "invalid script name".to_owned())?;
    let path = scripts_dir().join(format!("{stem}.py"));
    std::fs::write(&path, code).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Lists the names of saved scripts (excludes the `snippet-*` scratch files
/// that inline `/py` runs create and clean up).
#[must_use]
pub fn list_scripts() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(scripts_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "py") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    if !stem.starts_with("snippet-") {
                        names.push(stem.to_owned());
                    }
                }
            }
        }
    }
    names.sort();
    names
}

/// Resolves a `/run` argument to a script file: an existing path as-is,
/// otherwise a saved script by name (`~/.cyrene/scripts/<name>.py`).
#[must_use]
pub fn resolve_script(arg: &str) -> Option<PathBuf> {
    let direct = Path::new(arg);
    if direct.is_file() {
        return Some(direct.to_path_buf());
    }
    let stem = sanitize_name(arg)?;
    let named = scripts_dir().join(format!("{stem}.py"));
    named.is_file().then_some(named)
}

/// Runs Python source code by writing it to a temp file under [`scripts_dir`]
/// and executing it. `timeout` bounds the run; on expiry the child is killed
/// and `status` is `None`.
pub fn run_code(code: &str, timeout: Duration) -> Result<PyOutcome, String> {
    let py = interpreter().ok_or_else(|| {
        "No Python interpreter found. Install Python 3 (https://python.org) and try again."
            .to_owned()
    })?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = scripts_dir().join(format!("snippet-{secs}.py"));
    std::fs::write(&path, code).map_err(|e| format!("could not write script: {e}"))?;
    let outcome = run_file(py, &path, timeout);
    let _ = std::fs::remove_file(&path);
    outcome
}

/// Runs an existing Python file with the given interpreter.
pub fn run_file(py: &str, path: &Path, timeout: Duration) -> Result<PyOutcome, String> {
    use std::io::Read;

    let mut child = Command::new(py)
        .arg(path)
        .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {py}: {e}"))?;

    // Bounded wait: poll for completion, then kill if the timeout elapses.
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut stdout);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut stderr);
    }

    Ok(PyOutcome {
        status: status.and_then(|s| s.code()),
        stdout,
        stderr,
    })
}

/// A fenced Python block, with an optional skill name taken from the fence's
/// info string (e.g. ` ```python name=flights `). The name is how Cyrene marks
/// a block as a reusable skill she wants saved, without the user lifting a
/// finger.
#[derive(Debug, PartialEq)]
pub struct PyBlock {
    pub name: Option<String>,
    pub code: String,
}

/// Extracts fenced Python blocks (with any `name=` attribute) from markdown, so
/// the REPL can offer to run — and Cyrene can auto-save — scripts she writes.
/// Plain ``` blocks are ignored to avoid running non-Python output.
#[must_use]
pub fn extract_python_block_meta(text: &str) -> Vec<PyBlock> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let fence = line.trim_start();
        let info = fence
            .strip_prefix("```")
            .or_else(|| fence.strip_prefix("~~~"));
        let Some(info) = info else { continue };
        // The info string is `<lang> [attr=val ...]`, e.g. `python name=flights`.
        let mut toks = info.split_whitespace();
        let lang = toks.next().unwrap_or("");
        if !matches!(lang, "python" | "py" | "python3") {
            continue;
        }
        let mut name = None;
        for tok in toks {
            if let Some(v) = tok.strip_prefix("name=") {
                let v = v.trim_matches(['"', '\'']);
                if !v.is_empty() {
                    name = Some(v.to_owned());
                }
            }
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            let t = inner.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        if !body.trim().is_empty() {
            blocks.push(PyBlock { name, code: body });
        }
    }
    blocks
}

/// Just the code of every fenced Python block (names dropped). Backs the
/// run-the-script offer where the name doesn't matter.
#[must_use]
pub fn extract_python_blocks(text: &str) -> Vec<String> {
    extract_python_block_meta(text)
        .into_iter()
        .map(|b| b.code)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_python_fences_only() {
        let text = "Here you go:\n\n```python\nprint('hi')\n```\n\nand bash:\n```bash\nls\n```\n";
        let blocks = extract_python_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("print('hi')"));
    }

    #[test]
    fn captures_name_attribute_on_fence() {
        let text = "```python name=flights\nprint('hi')\n```\n```py\nx=1\n```";
        let blocks = extract_python_block_meta(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name.as_deref(), Some("flights"));
        assert!(blocks[0].code.contains("print('hi')"));
        assert_eq!(blocks[1].name, None);
        // Quotes around a name token are stripped.
        let q = extract_python_block_meta("```python name=\"weather_v2\"\na=1\n```");
        assert_eq!(q[0].name.as_deref(), Some("weather_v2"));
    }

    #[test]
    fn handles_py_and_python3_tags_and_tildes() {
        let text = "```py\na=1\n```\n~~~python3\nb=2\n~~~\n";
        assert_eq!(extract_python_blocks(text).len(), 2);
    }

    #[test]
    fn ignores_empty_and_plain_blocks() {
        assert!(extract_python_blocks("```\nplain\n```").is_empty());
        assert!(extract_python_blocks("```python\n\n```").is_empty());
    }

    #[test]
    fn sanitize_name_keeps_safe_chars() {
        assert_eq!(sanitize_name("Flight Scraper!").unwrap(), "flight-scraper");
        assert_eq!(sanitize_name("weather_v2").unwrap(), "weather_v2");
        assert_eq!(sanitize_name("../etc/passwd").unwrap(), "etc-passwd");
        assert!(sanitize_name("   ").is_none());
        assert!(sanitize_name("!!!").is_none());
    }

    #[test]
    fn summary_reports_exit_code() {
        let o = PyOutcome {
            status: Some(0),
            stdout: "hello\n".to_owned(),
            stderr: String::new(),
        };
        assert_eq!(o.status, Some(0));
        assert!(o.summary().contains("hello"));
        assert!(o.summary().contains("[exit 0]"));
    }

    // Runs only when a real interpreter is present; CI images without Python
    // still pass the suite.
    #[test]
    fn runs_real_python_when_available() {
        if interpreter().is_none() {
            return;
        }
        let o = run_code("print(6 * 7)", Duration::from_secs(10)).unwrap();
        assert_eq!(o.status, Some(0), "stderr: {}", o.stderr);
        assert_eq!(o.stdout.trim(), "42");
    }
}
