//! gray-notify — terminal/desktop notification when the agent settles.
//!
//! On every `turn_end` event the sidecar
//! emits a notification — OSC 99 for Kitty, OSC 777 for Ghostty/iTerm2/
//! WezTerm/rxvt (written to /dev/tty, since stdout is the JSON wire), a
//! Windows toast under `WT_SESSION`, and `notify-send` as a fallback or
//! when `GRAY_NOTIFY_SEND=1` asks for the desktop popup too.
//!
//! `/notify` reports status; `/notify test` fires one immediately.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn manifest() -> Value {
    json!({
        "name": "notify",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "tools": [],
        "commands": ["/notify"],
    })
}

/// Write a string to the controlling terminal, bypassing the NDJSON stdout
/// wire. Returns false when no tty is reachable (headless cron runs).
fn tty_write(s: &str) -> bool {
    // CONOUT$ is the Windows console device — same "controlling terminal,
    // bypassing stdout" semantics as /dev/tty on unix.
    #[cfg(windows)]
    const TTY: &str = "CONOUT$";
    #[cfg(not(windows))]
    const TTY: &str = "/dev/tty";
    std::fs::OpenOptions::new()
        .write(true)
        .open(TTY)
        .and_then(|mut f| f.write_all(s.as_bytes()).and_then(|_| f.flush()))
        .is_ok()
}

fn windows_toast(title: &str, body: &str) -> bool {
    let ty = "Windows.UI.Notifications";
    let script = format!(
        "[{ty}.ToastNotificationManager, {ty}, ContentType = WindowsRuntime] > $null; \
         $xml = [{ty}.ToastNotificationManager]::GetTemplateContent(\
         [{ty}.ToastTemplateType]::ToastText01); \
         $xml.GetElementsByTagName('text')[0].AppendChild($xml.CreateTextNode('{body}')) > $null; \
         [{ty}.ToastNotification]::CreateToastNotifier('{title}').Show($xml)"
    );
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

fn notify_send(title: &str, body: &str) -> bool {
    std::process::Command::new("notify-send")
        .args([title, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// macOS has no notify-send; osascript is the built-in desktop-popup path.
#[cfg(target_os = "macos")]
fn mac_notify(title: &str, body: &str) -> bool {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        esc(body),
        esc(title)
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(not(target_os = "macos"))]
fn mac_notify(_: &str, _: &str) -> bool {
    false
}

/// Escape OSC payloads: strip ESC and BEL so a hostile body can't break out.
fn osc_safe(s: &str) -> String {
    s.chars().filter(|c| *c != '\x1b' && *c != '\x07').collect()
}

/// Fire the notification through every channel that applies. Returns the
/// list of channels that succeeded (for `/notify` status output).
fn notify(title: &str, body: &str) -> Vec<&'static str> {
    let mut used = Vec::new();
    let title = osc_safe(title);
    let body = osc_safe(body);
    let _tty_ok = if std::env::var_os("WT_SESSION").is_some() {
        if windows_toast(&title, &body) {
            used.push("windows-toast");
            true
        } else {
            false
        }
    } else if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        if tty_write(&format!("\x1b]99;i=1:d=0;{title}\x1b\\"))
            && tty_write(&format!("\x1b]99;i=1:p=body;{body}\x1b\\"))
        {
            used.push("osc99");
            true
        } else {
            false
        }
    } else {
        if tty_write(&format!("\x1b]777;notify;{title};{body}\x07")) {
            used.push("osc777");
            true
        } else {
            false
        }
    };
    // Desktop popup only when explicitly enabled: headless runs (cron,
    // `gray -p` subagents) have no tty and would spam the desktop.
    let want_send = std::env::var("GRAY_NOTIFY_SEND")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    if want_send {
        if notify_send(&title, &body) {
            used.push("notify-send");
        } else if mac_notify(&title, &body) {
            used.push("osascript");
        }
    }
    used
}

fn on_turn_end() {
    notify("gray", "ready for input");
}

/// `/notify …` — `argv` excludes the command name.
fn run_command(argv: &[&str]) -> String {
    match argv.first().copied() {
        Some("test") => {
            let used = notify("gray-notify", "test notification");
            if used.is_empty() {
                "no notification channel worked (headless? try GRAY_NOTIFY_SEND=1)".into()
            } else {
                format!("sent via {}", used.join(", "))
            }
        }
        Some("on") | Some("off") => {
            let dir = dirs_state();
            let flag = dir.join("disabled");
            let res = if argv[0] == "off" {
                std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&flag, b""))
            } else {
                std::fs::remove_file(&flag).or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }
                })
            };
            match res {
                Ok(()) => format!("notifications {}", if argv[0] == "off" { "off" } else { "on" }),
                Err(e) => format!("couldn't flip state: {e}"),
            }
        }
        _ => format!(
            "gray-notify {} — notifies on turn_end via OSC 777/99, Windows toast, notify-send. \
             /notify test · /notify on|off",
            env!("CARGO_PKG_VERSION")
        ),
    }
}

fn dirs_state() -> std::path::PathBuf {
    let home = std::env::var_os("GRAY_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".gray")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join("notify")
}

fn enabled() -> bool {
    !dirs_state().join("disabled").exists()
}

/// One request → `Some(reply)`, or `None` for notifications. The bool asks
/// the loop to exit after writing the reply.
fn handle(req: &Value) -> (Option<Value>, bool) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        if method == "event/notify" && enabled() {
            if params.get("type").and_then(Value::as_str) == Some("turn_end") {
                on_turn_end();
            }
        }
        return (None, method == "plugin/shutdown");
    };
    let result = match method {
        "plugin/manifest" => manifest(),
        "command/run" => {
            let argv: Vec<&str> = params
                .get("argv")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            json!({ "text": run_command(&argv) })
        }
        "plugin/shutdown" => return (Some(json!({ "id": id, "result": {} })), true),
        _ => {
            let error = json!({ "code": -32601, "message": "method not found" });
            return (Some(json!({ "id": id, "error": error })), false);
        }
    };
    (Some(json!({ "id": id, "result": result })), false)
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest());
        return Ok(());
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        let (reply, exit) = handle(&req);
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
        if exit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, params: Value) -> Value {
        handle(&json!({ "id": 1, "method": method, "params": params })).0.unwrap()
    }

    #[test]
    fn manifest_has_no_tools_and_one_command() {
        let m = call("plugin/manifest", Value::Null)["result"].clone();
        assert_eq!(m["name"], "notify");
        assert_eq!(m["commands"], json!(["/notify"]));
        assert_eq!(m["tools"], json!([]));
    }

    #[test]
    fn notifications_are_silent_and_dont_exit() {
        let (reply, exit) = handle(&json!({
            "method": "event/notify",
            "params": {"type": "turn_end", "usage": {}, "session": {}}
        }));
        assert!(reply.is_none() && !exit);
    }

    #[test]
    fn shutdown_replies_then_exits() {
        let (reply, exit) = handle(&json!({ "id": 2, "method": "plugin/shutdown" }));
        assert!(reply.is_some() && exit);
    }

    #[test]
    fn osc_escapes_control_chars() {
        assert_eq!(osc_safe("a\x1b]8;;x\x07b"), "a]8;;xb");
    }

    #[test]
    fn notify_command_reports_version() {
        let r = call("command/run", json!({ "name": "/notify", "argv": [] }));
        assert!(r["result"]["text"].as_str().unwrap().contains("gray-notify"));
    }
}
