//! SQLite store: numbered migrations tracked by `PRAGMA user_version`, plus
//! the default profile created on first run (FR-GAME-08).

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_profiles.sql")];

const DEFAULT_PROFILE_NAME: &str = "เกมแรก";
/// Dialogue box at the bottom of a 16:9 frame, as fractions 0–1.
const DEFAULT_REGION: (f64, f64, f64, f64) = (0.125, 0.74, 0.75, 0.16);
/// Stream bar just above the dialogue box on 1920×1080.
const DEFAULT_BAR: (f64, f64, f64) = (0.1, 0.62, 0.8);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
    pub source: String,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    prepare(&conn)?;
    Ok(conn)
}

/// Migrate, then make sure at least one profile exists.
pub fn prepare(conn: &Connection) -> rusqlite::Result<()> {
    migrate(conn)?;
    seed_default_profile(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

fn seed_default_profile(conn: &Connection) -> rusqlite::Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let now = now_ms();
    let (rx, ry, rw, rh) = DEFAULT_REGION;
    let (bx, by, bw) = DEFAULT_BAR;
    conn.execute(
        "INSERT INTO profiles (id, name, region_x, region_y, region_w, region_h,
           bar_x, bar_y, bar_w, created_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            uuid::Uuid::new_v4().to_string(),
            DEFAULT_PROFILE_NAME,
            rx,
            ry,
            rw,
            rh,
            bx,
            by,
            bw,
            now
        ],
    )?;
    Ok(())
}

/// Most recently used profile; Yokoten opens with it (FR-GAME-08).
pub fn last_profile(conn: &Connection) -> rusqlite::Result<Option<ProfileRef>> {
    conn.query_row(
        "SELECT id, name, source FROM profiles ORDER BY last_used_at DESC LIMIT 1",
        [],
        |r| {
            Ok(ProfileRef {
                id: r.get(0)?,
                name: r.get(1)?,
                source: r.get(2)?,
            })
        },
    )
    .optional()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn first_run_creates_one_default_profile() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        assert_eq!(profile_count(&conn), 1);
        let p = last_profile(&conn).unwrap().unwrap();
        assert_eq!(p.name, DEFAULT_PROFILE_NAME);
        assert_eq!(p.source, "subtitle");
    }

    #[test]
    fn rerun_does_not_duplicate() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        prepare(&conn).unwrap();
        assert_eq!(profile_count(&conn), 1);
        let v: usize = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len());
    }

    #[test]
    fn default_profile_has_region_and_bar() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        let (rw, bw): (Option<f64>, Option<f64>) = conn
            .query_row("SELECT region_w, bar_w FROM profiles", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(rw, Some(DEFAULT_REGION.2));
        assert_eq!(bw, Some(DEFAULT_BAR.2));
    }

    #[test]
    fn file_db_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("yokoten-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("yokoten.db");
        let first = last_profile(&open(&path).unwrap()).unwrap().unwrap();
        let again = open(&path).unwrap();
        assert_eq!(profile_count(&again), 1);
        assert_eq!(last_profile(&again).unwrap().unwrap(), first);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
