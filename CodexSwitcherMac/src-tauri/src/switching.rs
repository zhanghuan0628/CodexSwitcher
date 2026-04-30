use rusqlite::{params, Connection};

use crate::{now_text, Account, SwitchLog};

pub(crate) fn is_switchable(account: &Account) -> Result<(), String> {
    if account.is_real_session && account.binding_kind != "codex_cli" {
        return Err("当前仅支持通过官方 CLI 绑定的真实账号切换。".to_string());
    }

    if account.auth_state == "mismatch" {
        return Err("目标账号与当前官方登录态不一致，请先重新绑定。".to_string());
    }

    if account.auth_state == "expired" || account.status == "auth_invalid" {
        return Err("目标账号登录已失效，请先重新登录或重新绑定。".to_string());
    }

    if account.auth_state == "unknown" {
        return Err("目标账号尚未完成有效校验，请先验证账号。".to_string());
    }

    if account.auth_state != "valid" {
        return Err("目标账号授权状态异常，请先重新绑定。".to_string());
    }

    match account.status.as_str() {
        "healthy" | "warning" => Ok(()),
        "exhausted" => Err("目标账号当前已耗尽，暂时不可切换".to_string()),
        "error" => Err("目标账号检测异常，请先修复后再切换".to_string()),
        _ => Err("目标账号当前不可切换，请先修复状态或等待恢复".to_string()),
    }
}

pub(crate) fn set_active_account(
    connection: &Connection,
    target_account_id: i64,
) -> Result<(), String> {
    connection
        .execute("UPDATE accounts SET is_active = 0", [])
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE accounts SET is_active = 1, updated_at = ?1 WHERE id = ?2",
            params![now_text(), target_account_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn rollback_active_account(
    connection: &Connection,
    previous_account_id: Option<i64>,
    ensure_one_active: impl FnOnce(&Connection) -> Result<(), String>,
) -> Result<(), String> {
    connection
        .execute("UPDATE accounts SET is_active = 0", [])
        .map_err(|error| error.to_string())?;

    if let Some(previous_id) = previous_account_id {
        connection
            .execute(
                "UPDATE accounts SET is_active = 1, updated_at = ?1 WHERE id = ?2",
                params![now_text(), previous_id],
            )
            .map_err(|error| error.to_string())?;
    } else {
        ensure_one_active(connection)?;
    }

    Ok(())
}

pub(crate) fn insert_switch_log(
    connection: &Connection,
    from_account_id: Option<i64>,
    to_account_id: i64,
    result: &str,
    reason: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO switch_logs (from_account_id, to_account_id, result, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![from_account_id, to_account_id, result, reason, now_text()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn query_switch_logs(connection: &Connection) -> Result<Vec<SwitchLog>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, from_account_id, to_account_id, result, reason, created_at
             FROM switch_logs ORDER BY id DESC LIMIT 8",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(SwitchLog {
                id: row.get(0)?,
                from_account_id: row.get(1)?,
                to_account_id: row.get(2)?,
                result: row.get(3)?,
                reason: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn latest_switch_for_account(
    connection: &Connection,
    account_id: i64,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT created_at FROM switch_logs
             WHERE from_account_id = ?1 OR to_account_id = ?1
             ORDER BY id DESC LIMIT 1",
            [account_id],
            |row| row.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other.to_string()),
        })
}

pub(crate) fn query_recent_switches_for_account(
    connection: &Connection,
    account_id: i64,
    limit: i64,
) -> Result<Vec<SwitchLog>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, from_account_id, to_account_id, result, reason, created_at
             FROM switch_logs
             WHERE from_account_id = ?1 OR to_account_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map(params![account_id, limit], |row| {
            Ok(SwitchLog {
                id: row.get(0)?,
                from_account_id: row.get(1)?,
                to_account_id: row.get(2)?,
                result: row.get(3)?,
                reason: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}
