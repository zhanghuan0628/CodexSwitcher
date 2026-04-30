use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StartupHealthCheck {
    pub(crate) label: String,
    pub(crate) ok: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct StartupHealth {
    pub(crate) generated_at: String,
    pub(crate) healthy: bool,
    pub(crate) checks: Vec<StartupHealthCheck>,
}

pub(crate) fn build_startup_health(
    connection: &Connection,
    app_data_dir: &Path,
    generated_at: String,
) -> StartupHealth {
    let mut checks = Vec::new();

    let app_dir_ok = app_data_dir.exists() && app_data_dir.is_dir();
    checks.push(StartupHealthCheck {
        label: "应用数据目录".to_string(),
        ok: app_dir_ok,
        detail: app_data_dir.display().to_string(),
    });

    match connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0)) {
        Ok(result) => checks.push(StartupHealthCheck {
            label: "SQLite quick_check".to_string(),
            ok: result.eq_ignore_ascii_case("ok"),
            detail: result,
        }),
        Err(error) => checks.push(StartupHealthCheck {
            label: "SQLite quick_check".to_string(),
            ok: false,
            detail: error.to_string(),
        }),
    }

    for table in [
        "accounts",
        "usage_snapshots",
        "switch_logs",
        "handoff_cards",
        "notifications",
        "app_settings",
    ] {
        let exists = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);
        checks.push(StartupHealthCheck {
            label: format!("数据表 {}", table),
            ok: exists,
            detail: if exists {
                "存在".to_string()
            } else {
                "缺失".to_string()
            },
        });
    }

    let active_count = connection
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_active = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    checks.push(StartupHealthCheck {
        label: "活跃账号数量".to_string(),
        ok: active_count <= 1,
        detail: format!("{} 个", active_count),
    });

    let settings_count = connection
        .query_row(
            "SELECT COUNT(*) FROM app_settings WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    checks.push(StartupHealthCheck {
        label: "基础设置记录".to_string(),
        ok: settings_count == 1,
        detail: format!("{} 条", settings_count),
    });

    let healthy = checks.iter().all(|check| check.ok);
    StartupHealth {
        generated_at,
        healthy,
        checks,
    }
}
