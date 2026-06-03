use dirs::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const CODEX_COMMAND_TIMEOUT_SECS: u64 = 8;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct CodexAuthStatus {
    pub(crate) logged_in: bool,
    pub(crate) auth_mode: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) account_email: Option<String>,
    pub(crate) config_dir: String,
    pub(crate) auth_path: Option<String>,
    pub(crate) session_ref: Option<String>,
    pub(crate) session_json: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct BindEnvironmentDiagnostic {
    pub(crate) codex_config_dir: String,
    pub(crate) auth_path: Option<String>,
    pub(crate) auth_exists: bool,
    pub(crate) current_email: Option<String>,
    pub(crate) current_account_id: Option<String>,
    pub(crate) current_is_bound: bool,
    pub(crate) cli_candidates: Vec<String>,
    pub(crate) cli_available: Option<String>,
    pub(crate) cli_status_ok: bool,
    pub(crate) cli_stdout: String,
    pub(crate) cli_stderr: String,
}

pub(crate) fn codex_config_dir() -> Result<PathBuf, String> {
    if let Ok(value) = env::var("CODEX_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    let home = home_dir().ok_or_else(|| "无法定位用户目录，无法读取 Codex 登录态".to_string())?;
    Ok(home.join(".codex"))
}

fn auth_path_from_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("auth.json")
}

fn command_with_gui_env(program: &Path) -> Command {
    let mut command = Command::new(program);
    let mut path_entries = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        "/usr/sbin".to_string(),
        "/sbin".to_string(),
    ];

    if let Ok(current_path) = env::var("PATH") {
        for item in current_path.split(':') {
            let trimmed = item.trim();
            if !trimmed.is_empty() && !path_entries.iter().any(|entry| entry == trimmed) {
                path_entries.push(trimmed.to_string());
            }
        }
    }

    command.env("PATH", path_entries.join(":"));
    command
}

fn command_output_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started_at = Instant::now();

    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }

        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("命令执行超过 {} 秒", timeout.as_secs()),
            ));
        }

        thread::sleep(Duration::from_millis(50));
    }
}

fn codex_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(value) = env::var("CODEX_CLI_PATH") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    candidates.push(PathBuf::from(
        "/Applications/Codex.app/Contents/Resources/codex",
    ));
    candidates.push(PathBuf::from("/opt/homebrew/bin/codex"));
    candidates.push(PathBuf::from("/usr/local/bin/codex"));
    candidates.push(PathBuf::from("codex"));
    candidates
}

fn resolve_command_to_absolute_path(command_name: &str) -> Option<PathBuf> {
    let output = command_output_with_timeout(
        command_with_gui_env(Path::new("/bin/zsh"))
            .args(["-lc", &format!("command -v {}", command_name)]),
        Duration::from_secs(CODEX_COMMAND_TIMEOUT_SECS),
    )
    .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(PathBuf::from(stdout))
    }
}

pub(crate) fn resolve_codex_cli_path() -> Result<PathBuf, String> {
    let mut last_error = String::new();

    for candidate in codex_cli_candidates() {
        match command_output_with_timeout(
            command_with_gui_env(&candidate).arg("--help"),
            Duration::from_secs(CODEX_COMMAND_TIMEOUT_SECS),
        ) {
            Ok(output) if output.status.success() => {
                if candidate.is_absolute() {
                    return Ok(candidate);
                }

                return resolve_command_to_absolute_path("codex").ok_or_else(|| {
                    "已检测到 codex 命令可用，但无法解析为 Terminal 可执行的绝对路径".to_string()
                });
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                last_error = if stderr.is_empty() {
                    format!("{}：命令返回非零状态", candidate.display())
                } else {
                    format!("{}：{}", candidate.display(), stderr)
                };
            }
            Err(error) => {
                last_error = format!("{}：{}", candidate.display(), error);
            }
        }
    }

    if last_error.is_empty() {
        Err("未找到可用的 codex 命令".to_string())
    } else {
        Err(format!("未找到可用的 codex 命令：{}", last_error))
    }
}

fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn decode_base64_url(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => continue,
            _ => return None,
        } as u32;

        buffer = (buffer << 6) | value;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
            buffer &= (1 << bits) - 1;
        }
    }

    Some(output)
}

fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = decode_base64_url(payload)?;
    serde_json::from_slice::<Value>(&bytes).ok()
}

fn extract_auth_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn extract_codex_identity_from_session_json(
    session_json: &str,
) -> (Option<String>, Option<String>) {
    let auth_value = serde_json::from_str::<Value>(session_json).unwrap_or(Value::Null);
    let account_id = extract_auth_string(&auth_value, &["tokens", "account_id"]);
    let account_email = extract_auth_string(&auth_value, &["tokens", "id_token"])
        .and_then(|token| decode_jwt_payload(&token))
        .and_then(|payload| extract_auth_string(&payload, &["email"]));
    (account_id, account_email)
}

fn read_codex_auth_file_status() -> Result<CodexAuthStatus, String> {
    let config_dir = codex_config_dir()?;
    let auth_path = auth_path_from_dir(&config_dir);
    let auth_path_value = auth_path
        .exists()
        .then(|| auth_path.to_string_lossy().to_string());
    let session_json = if auth_path.exists() {
        fs::read_to_string(&auth_path)
            .map_err(|error| format!("读取 Codex 登录凭证失败：{}", error))?
    } else {
        String::new()
    };
    let auth_value = serde_json::from_str::<Value>(&session_json).unwrap_or(Value::Null);
    let auth_mode = auth_value
        .get("auth_mode")
        .and_then(|item| item.as_str())
        .map(|item| item.to_string());
    let (account_id, account_email) = extract_codex_identity_from_session_json(&session_json);
    let logged_in = auth_path_value.is_some() && auth_mode.as_deref() == Some("chatgpt");
    let session_ref = auth_path_value.clone();

    Ok(CodexAuthStatus {
        logged_in,
        auth_mode,
        account_id,
        account_email,
        config_dir: config_dir.to_string_lossy().to_string(),
        auth_path: auth_path_value,
        session_ref,
        session_json,
    })
}

pub(crate) fn read_codex_auth_status_cached() -> Result<CodexAuthStatus, String> {
    read_codex_auth_file_status()
}

pub(crate) fn read_codex_auth_status() -> Result<CodexAuthStatus, String> {
    let mut file_status = read_codex_auth_file_status()?;

    let mut last_error = String::new();
    let mut output = None;

    for candidate in codex_cli_candidates() {
        match command_output_with_timeout(
            command_with_gui_env(&candidate).args(["login", "status"]),
            Duration::from_secs(CODEX_COMMAND_TIMEOUT_SECS),
        ) {
            Ok(result)
                if String::from_utf8_lossy(&result.stdout)
                    .to_ascii_lowercase()
                    .contains("logged in") =>
            {
                output = Some(result);
                break;
            }
            Ok(result) => {
                last_error = String::from_utf8_lossy(&result.stderr).trim().to_string();
                output = Some(result);
            }
            Err(error) => {
                last_error = format!("{}：{}", candidate.display(), error);
            }
        }
    }

    if output.is_none() && file_status.auth_path.is_none() {
        return Err(if last_error.is_empty() {
            "执行 codex login status 失败：未找到可用的 codex 命令".to_string()
        } else {
            format!("执行 codex login status 失败：{}", last_error)
        });
    }

    if let Some(output) = output.as_ref() {
        if !output.status.success() && file_status.auth_path.is_none() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "codex login status 执行失败".to_string()
            } else {
                format!("codex login status 执行失败：{}", stderr)
            });
        }
    }

    let raw = output
        .as_ref()
        .map(|item| String::from_utf8_lossy(&item.stdout).to_string())
        .unwrap_or_default();
    let cli_logged_in = raw.to_ascii_lowercase().contains("logged in");
    let auth_json_logged_in = file_status.logged_in;
    let logged_in = cli_logged_in || auth_json_logged_in;
    let session_json = if file_status.session_json.trim().is_empty() {
        raw.clone()
    } else {
        file_status.session_json.clone()
    };
    let session_ref = file_status
        .auth_path
        .clone()
        .or_else(|| Some("codex-login-status://current".to_string()));

    file_status.logged_in = logged_in;
    file_status.session_ref = session_ref;
    file_status.session_json = session_json;
    Ok(file_status)
}

pub(crate) fn diagnose_bind_environment() -> Result<BindEnvironmentDiagnostic, String> {
    let config_dir = codex_config_dir()?;
    let auth_path = auth_path_from_dir(&config_dir);
    let status = read_codex_auth_status().ok();
    let cli_candidates = codex_cli_candidates();
    let mut cli_available = None;
    let mut cli_status_ok = false;
    let mut cli_stdout = String::new();
    let mut cli_stderr = String::new();

    for candidate in &cli_candidates {
        match command_output_with_timeout(
            command_with_gui_env(candidate).args(["login", "status"]),
            Duration::from_secs(CODEX_COMMAND_TIMEOUT_SECS),
        ) {
            Ok(output) => {
                cli_available = Some(candidate.display().to_string());
                cli_status_ok = output.status.success();
                cli_stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                cli_stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                break;
            }
            Err(error) => {
                cli_stderr = format!("{}：{}", candidate.display(), error);
            }
        }
    }

    Ok(BindEnvironmentDiagnostic {
        codex_config_dir: config_dir.to_string_lossy().to_string(),
        auth_path: Some(auth_path.to_string_lossy().to_string()),
        auth_exists: auth_path.exists(),
        current_email: status.as_ref().and_then(|item| item.account_email.clone()),
        current_account_id: status.as_ref().and_then(|item| item.account_id.clone()),
        current_is_bound: false,
        cli_candidates: cli_candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        cli_available,
        cli_status_ok,
        cli_stdout,
        cli_stderr,
    })
}

pub(crate) fn start_codex_login_flow() -> Result<String, String> {
    let cli_path = resolve_codex_cli_path()?;
    let login_command = format!(
        "clear; echo 'CodexSwitcher 正在打开 Codex 官方登录流程...'; echo ''; \"{}\" login; exit_code=$?; echo ''; if [ $exit_code -eq 0 ]; then echo 'Codex 官方登录流程已结束。请回到 CodexSwitcher 点击“绑定当前已登录账号”或“刷新状态”。'; else echo 'Codex 登录流程未成功完成，请检查上面的输出后重试。'; fi; exec zsh -l",
        cli_path.display()
    );
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"",
        escape_applescript_string(&login_command)
    );

    let output = command_output_with_timeout(
        Command::new("osascript").args([
            "-e",
            &script,
            "-e",
            "tell application \"Terminal\" to activate",
        ]),
        Duration::from_secs(CODEX_COMMAND_TIMEOUT_SECS),
    )
    .map_err(|error| format!("打开官方登录流程失败：{}", error))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "打开官方登录流程失败".to_string()
        } else {
            format!("打开官方登录流程失败：{}", stderr)
        });
    }

    Ok("已在 Terminal 中打开 Codex 官方登录流程，登录完成后请回到应用绑定当前账号。".to_string())
}

pub(crate) fn verify_real_codex_session() -> Result<CodexAuthStatus, String> {
    let status = read_codex_auth_status()?;
    if !status.logged_in {
        return Err("当前未检测到已登录的 Codex 官方会话，请先完成 Codex 官方登录。".to_string());
    }
    if status.auth_mode.as_deref() != Some("chatgpt") {
        return Err(
            "当前不是 Codex ChatGPT 账号登录态，请执行 codex login 完成官方账号登录。".to_string(),
        );
    }

    Ok(status)
}
