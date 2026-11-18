//! SQLite store: numbered migrations tracked by `PRAGMA user_version`, plus
//! the default profile created on first run (FR-GAME-08).

use crate::core::obs::Region;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_profiles.sql"),
    include_str!("../migrations/0002_lines.sql"),
];

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
    conn.pragma_update(None, "foreign_keys", true)?;
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

/// Subtitle box of a profile; None until the profile has one (FR-REG).
pub fn profile_region(conn: &Connection, profile_id: &str) -> rusqlite::Result<Option<Region>> {
    conn.query_row(
        "SELECT region_x, region_y, region_w, region_h FROM profiles WHERE id = ?1",
        [profile_id],
        |r| {
            Ok(match (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?) {
                (Some(x), Some(y), Some(w), Some(h)) => Some(Region { x, y, w, h }),
                _ => None,
            })
        },
    )
    .optional()
    .map(Option::flatten)
}

/// One translated line (SRS 7.3 / 8.4 `Line`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Line {
    pub id: String,
    pub profile_id: String,
    pub session_id: String,
    pub source: String,
    pub source_text: String,
    pub target_text: String,
    pub engine: String,
    pub is_fallback: bool,
    pub model_id: Option<String>,
    /// false while the audio path is still hearing the sentence.
    #[serde(rename = "final")]
    pub done: bool,
    pub created_at: i64,
    pub ocr_ms: Option<i64>,
    pub translate_ms: Option<i64>,
}

pub fn insert_line(conn: &Connection, l: &Line) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO lines (id, profile_id, session_id, source, source_text, target_text,
           engine, is_fallback, model_id, ocr_ms, translate_ms, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            l.id,
            l.profile_id,
            l.session_id,
            l.source,
            l.source_text,
            l.target_text,
            l.engine,
            l.is_fallback,
            l.model_id,
            l.ocr_ms,
            l.translate_ms,
            l.created_at
        ],
    )?;
    Ok(())
}

/// Rewrite a line in place (typewriter subtitle grew, or retranslated).
pub fn update_line(conn: &Connection, l: &Line) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE lines SET source_text = ?2, target_text = ?3, engine = ?4,
           is_fallback = ?5, model_id = ?6, translate_ms = ?7 WHERE id = ?1",
        params![
            l.id,
            l.source_text,
            l.target_text,
            l.engine,
            l.is_fallback,
            l.model_id,
            l.translate_ms
        ],
    )?;
    Ok(())
}

/// A page of one session's history, newest first (FR-WEB-02, NFR-10).
pub fn session_lines(
    conn: &Connection,
    session_id: &str,
    before: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<Line>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_id, session_id, source, source_text, target_text, engine,
                is_fallback, model_id, created_at, ocr_ms, translate_ms
         FROM lines WHERE session_id = ?1 AND created_at < ?2
         ORDER BY created_at DESC, rowid DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![session_id, before.unwrap_or(i64::MAX), limit],
        |r| {
            Ok(Line {
                id: r.get(0)?,
                profile_id: r.get(1)?,
                session_id: r.get(2)?,
                source: r.get(3)?,
                source_text: r.get(4)?,
                target_text: r.get(5)?,
                engine: r.get(6)?,
                is_fallback: r.get(7)?,
                model_id: r.get(8)?,
                done: true,
                created_at: r.get(9)?,
                ocr_ms: r.get(10)?,
                translate_ms: r.get(11)?,
            })
        },
    )?;
    rows.collect()
}

pub fn now_ms() -> i64 {
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
    fn profile_region_reads_fractions() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        let id = last_profile(&conn).unwrap().unwrap().id;
        let (x, y, w, h) = DEFAULT_REGION;
        assert_eq!(
            profile_region(&conn, &id).unwrap(),
            Some(Region { x, y, w, h })
        );
        assert_eq!(profile_region(&conn, "nope").unwrap(), None);
        conn.execute("UPDATE profiles SET region_w = NULL", [])
            .unwrap();
        assert_eq!(profile_region(&conn, &id).unwrap(), None);
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

    fn line(conn: &Connection, session: &str, text: &str, at: i64) -> Line {
        let l = Line {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: last_profile(conn).unwrap().unwrap().id,
            session_id: session.into(),
            source: "subtitle".into(),
            source_text: text.into(),
            target_text: format!("th {text}"),
            engine: "fake".into(),
            is_fallback: false,
            model_id: None,
            done: true,
            created_at: at,
            ocr_ms: Some(40),
            translate_ms: Some(1),
        };
        insert_line(conn, &l).unwrap();
        l
    }

    #[test]
    fn session_lines_pages_newest_first_within_session() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        let a = line(&conn, "s1", "a", 1);
        let b = line(&conn, "s1", "b", 2);
        let c = line(&conn, "s1", "c", 3);
        line(&conn, "s2", "other", 4);
        assert_eq!(
            session_lines(&conn, "s1", None, 10).unwrap(),
            vec![c.clone(), b.clone(), a.clone()]
        );
        assert_eq!(
            session_lines(&conn, "s1", None, 2).unwrap(),
            vec![c, b.clone()]
        );
        assert_eq!(session_lines(&conn, "s1", Some(2), 10).unwrap(), vec![a]);
    }

    #[test]
    fn update_line_rewrites_text() {
        let conn = Connection::open_in_memory().unwrap();
        prepare(&conn).unwrap();
        let mut l = line(&conn, "s1", "No. But", 1);
        l.source_text = "No. But it's the end.".into();
        l.target_text = "ไม่ แต่มันจบแล้ว".into();
        update_line(&conn, &l).unwrap();
        assert_eq!(session_lines(&conn, "s1", None, 1).unwrap(), vec![l]);
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
