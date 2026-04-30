use std::process::Command;

const KEYCHAIN_SERVICE_NAME: &str = "com.codexswitcher.mac.codex";

pub(crate) fn keychain_session_ref(account_key: &str) -> String {
    format!("keychain://{}", account_key)
}

pub(crate) fn keychain_account_key(session_ref: &str) -> Option<&str> {
    session_ref.strip_prefix("keychain://")
}

pub(crate) fn trim_security_output(mut value: String) -> String {
    while value.ends_with(['\n', '\r']) {
        value.pop();
    }
    value
}

pub(crate) fn decode_hex_payload_if_needed(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && trimmed.len() % 2 == 0
        && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        if let Ok(bytes) = (0..trimmed.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&trimmed[index..index + 2], 16))
            .collect::<Result<Vec<_>, _>>()
        {
            if let Ok(decoded) = String::from_utf8(bytes) {
                return decoded;
            }
        }
    }

    value
}

pub(crate) fn store_account_secret(
    account_key: &str,
    credentials_json: &str,
) -> Result<String, String> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE_NAME,
            "-a",
            account_key,
            "-w",
            credentials_json,
        ])
        .output()
        .map_err(|error| format!("写入 macOS Keychain 失败：{}", error))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "写入 macOS Keychain 失败".to_string()
        } else {
            format!("写入 macOS Keychain 失败：{}", stderr)
        });
    }

    Ok(keychain_session_ref(account_key))
}

pub(crate) fn load_account_secret(account_key: &str) -> Result<String, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE_NAME,
            "-a",
            account_key,
            "-w",
        ])
        .output()
        .map_err(|error| format!("读取 macOS Keychain 失败：{}", error))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("未找到账号 {} 的 Keychain 会话", account_key)
        } else {
            format!("读取 macOS Keychain 失败：{}", stderr)
        });
    }

    let credentials_json = decode_hex_payload_if_needed(trim_security_output(
        String::from_utf8_lossy(&output.stdout).to_string(),
    ));
    if credentials_json.trim().is_empty() {
        return Err("Keychain 中的账号会话为空，无法执行真实切换。".to_string());
    }

    Ok(credentials_json)
}

pub(crate) fn delete_account_secret(account_key: &str) -> Result<(), String> {
    let output = Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE_NAME,
            "-a",
            account_key,
        ])
        .output()
        .map_err(|error| format!("删除 macOS Keychain 会话失败：{}", error))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("could not be found")
        || stderr.contains("The specified item could not be found")
    {
        return Ok(());
    }

    Err(if stderr.is_empty() {
        "删除 macOS Keychain 会话失败".to_string()
    } else {
        format!("删除 macOS Keychain 会话失败：{}", stderr)
    })
}
