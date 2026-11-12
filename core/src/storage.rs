//! SQLite 持久层（rusqlite bundled）。
//!
//! 只存三类东西：曲目元数据、下载记录、收藏。搜索/详情/取链走内存 TTL 缓存。

use crate::error::Result;
use crate::model::{PlaylistRef, Quality, Session, Track};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct Storage {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadRow {
    pub task_id: String,
    pub provider: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub path: String,
    pub quality: String,
    pub bytes: i64,
    pub status: String,
    pub created_at: i64,
}

/// 下载里失败的一个部分：`audio` / `lyrics` / `cover`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailedPart {
    pub part: String,
    pub error: String,
}

/// 一条「下载不完整」记录。
///
/// 同一首歌 + 同一个目标只留一条：重试成功就删掉，重试仍失败就原地更新。
/// 音频成功、只是封面/歌词失败时 `location` 指向已经落盘的文件 / 网盘对象，
/// 手动重试可以只补那部分，不用整首重下。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FailedDownload {
    pub id: String,
    pub track: Track,
    pub quality: String,
    /// `local` / `s3`
    pub target: String,
    /// 网盘歌单名（本地下载为空）
    pub playlist: String,
    /// 本地文件路径或网盘对象 key；音频都没下成功时为空
    pub location: String,
    pub parts: Vec<FailedPart>,
    pub created_at: i64,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS tracks (
  provider TEXT NOT NULL, id TEXT NOT NULL, title TEXT, artist TEXT, artist_id TEXT,
  album TEXT, album_id TEXT, duration INTEGER, cover TEXT,
  qualities TEXT, has_advanced INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (provider, id)
);
CREATE TABLE IF NOT EXISTS downloads (
  task_id TEXT PRIMARY KEY, provider TEXT, id TEXT, title TEXT, artist TEXT,
  path TEXT, quality TEXT, bytes INTEGER, status TEXT, created_at INTEGER,
  track_json TEXT
);
CREATE TABLE IF NOT EXISTS download_failures (
  id TEXT PRIMARY KEY, track TEXT NOT NULL, quality TEXT, target TEXT,
  playlist TEXT, location TEXT, parts TEXT, created_at INTEGER
);
CREATE TABLE IF NOT EXISTS favorites (
  provider TEXT NOT NULL, id TEXT NOT NULL, added_at INTEGER,
  PRIMARY KEY (provider, id)
);
CREATE TABLE IF NOT EXISTS sessions (
  provider TEXT PRIMARY KEY, user_id TEXT, nickname TEXT, avatar TEXT,
  cookies TEXT, extra TEXT, created_at INTEGER
);
CREATE TABLE IF NOT EXISTS saved_playlists (
  provider TEXT NOT NULL, playlist_id TEXT NOT NULL, name TEXT, cover TEXT,
  track_count INTEGER, creator TEXT, added_at INTEGER,
  PRIMARY KEY (provider, playlist_id)
);
CREATE TABLE IF NOT EXISTS s3_playlists (
  name TEXT PRIMARY KEY, created_at INTEGER,
  sort_order INTEGER NOT NULL DEFAULT 0
);
"#;

/// 老库缺列时补上（SQLite 没有 ADD COLUMN IF NOT EXISTS）
fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) {
    let exists: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM pragma_table_info('{}') WHERE name=?1",
                table
            ),
            [column],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        let _ = conn.execute_batch(ddl);
    }
}

/// 一次性数据迁移。用 `PRAGMA user_version` 防止每次启动都改用户数据。
fn migrate(conn: &Connection) {
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if v < 1 {
        let _ = conn.execute(
            "UPDATE downloads SET quality='standard' WHERE quality='high'",
            [],
        );
        let _ = conn.pragma_update(None, "user_version", 1);
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        ensure_column(
            &conn,
            "s3_playlists",
            "sort_order",
            "ALTER TABLE s3_playlists ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
        );
        ensure_column(
            &conn,
            "tracks",
            "qualities",
            "ALTER TABLE tracks ADD COLUMN qualities TEXT",
        );
        ensure_column(
            &conn,
            "tracks",
            "has_advanced",
            "ALTER TABLE tracks ADD COLUMN has_advanced INTEGER NOT NULL DEFAULT 0",
        );
        ensure_column(
            &conn,
            "downloads",
            "track_json",
            "ALTER TABLE downloads ADD COLUMN track_json TEXT",
        );
        migrate(&conn);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        ensure_column(
            &conn,
            "s3_playlists",
            "sort_order",
            "ALTER TABLE s3_playlists ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
        );
        ensure_column(
            &conn,
            "tracks",
            "qualities",
            "ALTER TABLE tracks ADD COLUMN qualities TEXT",
        );
        ensure_column(
            &conn,
            "tracks",
            "has_advanced",
            "ALTER TABLE tracks ADD COLUMN has_advanced INTEGER NOT NULL DEFAULT 0",
        );
        ensure_column(
            &conn,
            "downloads",
            "track_json",
            "ALTER TABLE downloads ADD COLUMN track_json TEXT",
        );
        migrate(&conn);
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_track(&self, t: &Track) -> Result<()> {
        let qualities =
            serde_json::to_string(&t.available_qualities).unwrap_or_else(|_| "[]".into());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracks (provider,id,title,artist,artist_id,album,album_id,duration,cover,qualities,has_advanced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(provider,id) DO UPDATE SET
               title=excluded.title, artist=excluded.artist, artist_id=excluded.artist_id,
               album=excluded.album, album_id=excluded.album_id,
               duration=excluded.duration, cover=excluded.cover,
               qualities=excluded.qualities, has_advanced=excluded.has_advanced",
            params![
                t.provider, t.id, t.title, t.artist, t.artist_id,
                t.album, t.album_id, t.duration_sec, t.cover,
                qualities, t.has_advanced as i64
            ],
        )?;
        Ok(())
    }

    pub fn add_favorite(&self, t: &Track) -> Result<()> {
        self.upsert_track(t)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO favorites (provider,id,added_at) VALUES (?1,?2,?3)",
            params![t.provider, t.id, now_ts()],
        )?;
        Ok(())
    }

    pub fn remove_favorite(&self, provider: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM favorites WHERE provider=?1 AND id=?2",
            params![provider, id],
        )?;
        Ok(())
    }

    pub fn list_favorites(&self) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.provider,t.id,t.title,t.artist,t.artist_id,t.album,t.album_id,t.duration,t.cover,t.qualities,t.has_advanced
             FROM favorites f JOIN tracks t ON t.provider=f.provider AND t.id=f.id
             ORDER BY f.added_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let raw: Option<String> = r.get(9).ok();
            let available_qualities = raw
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<Quality>>(s).ok())
                .unwrap_or_default();
            Ok(Track {
                provider: r.get(0)?,
                id: r.get(1)?,
                title: r.get(2)?,
                artist: r.get(3)?,
                artist_id: r.get(4)?,
                album: r.get(5)?,
                album_id: r.get(6)?,
                duration_sec: r.get::<_, i64>(7).unwrap_or(0) as u32,
                cover: r.get(8)?,
                hot: 0,
                availability: crate::model::Availability::Unknown,
                subtitle: None,
                available_qualities,
                has_advanced: r.get::<_, i64>(10).unwrap_or(0) != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn is_favorite(&self, provider: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM favorites WHERE provider=?1 AND id=?2",
            params![provider, id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn insert_download(
        &self,
        task_id: &str,
        t: &Track,
        quality: Quality,
        path: &str,
        bytes: i64,
        status: &str,
    ) -> Result<()> {
        self.upsert_track(t)?;
        // 完整曲目留一份：网盘上传后如果资源缺了，还能按 key 找回原平台信息补齐
        let track_json = serde_json::to_string(t).unwrap_or_default();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO downloads
             (task_id,provider,id,title,artist,path,quality,bytes,status,created_at,track_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                task_id,
                t.provider,
                t.id,
                t.title,
                t.artist,
                path,
                quality.as_str(),
                bytes,
                status,
                now_ts(),
                track_json
            ],
        )?;
        Ok(())
    }

    /// 网盘上传记录 → 源曲目（`s3://` 路径 → Track），用于完整性检测里「能不能补齐」。
    ///
    /// 老记录没有 track_json，用 provider/id/title/artist 拼一个够用的壳：
    /// 歌词按 id 直接能取，封面再按平台搜一次补上。
    pub fn list_s3_sources(&self) -> Result<Vec<(String, Track)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT path, track_json, provider, id, title, artist
             FROM downloads
             WHERE status='done' AND path LIKE 's3://%'
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(0)?;
            let json: Option<String> = r.get(1)?;
            let provider: String = r.get(2)?;
            let id: String = r.get(3)?;
            let title: String = r.get(4)?;
            let artist: String = r.get(5)?;
            let track = json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Track>(s).ok())
                .unwrap_or_else(|| Track {
                    provider,
                    id,
                    title,
                    artist,
                    ..Track::default()
                });
            Ok((path, track))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn update_download(&self, task_id: &str, status: &str, bytes: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET status=?2, bytes=?3 WHERE task_id=?1",
            params![task_id, status, bytes],
        )?;
        Ok(())
    }

    /// 清空下载记录（不删除已下载的文件）
    pub fn clear_downloads(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM downloads", [])?;
        Ok(())
    }

    // ---------- 下载失败记录 ----------

    /// 写入/更新一条失败记录（同一首歌 + 同一目标复用 `id`）
    pub fn upsert_download_failure(&self, f: &FailedDownload) -> Result<()> {
        let track = serde_json::to_string(&f.track).unwrap_or_default();
        let parts = serde_json::to_string(&f.parts).unwrap_or_default();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO download_failures
             (id,track,quality,target,playlist,location,parts,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                f.id,
                track,
                f.quality,
                f.target,
                f.playlist,
                f.location,
                parts,
                if f.created_at > 0 { f.created_at } else { now_ts() }
            ],
        )?;
        Ok(())
    }

    pub fn list_download_failures(&self) -> Result<Vec<FailedDownload>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,track,quality,target,playlist,location,parts,created_at
             FROM download_failures ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let track: String = r.get(1)?;
            let parts: String = r.get(6)?;
            Ok(FailedDownload {
                id: r.get(0)?,
                track: serde_json::from_str(&track).unwrap_or_default(),
                quality: r.get(2)?,
                target: r.get(3)?,
                playlist: r.get(4)?,
                location: r.get(5)?,
                parts: serde_json::from_str(&parts).unwrap_or_default(),
                created_at: r.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn remove_download_failure(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM download_failures WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn clear_download_failures(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM download_failures", [])?;
        Ok(())
    }

    pub fn list_downloads(&self) -> Result<Vec<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT task_id,provider,id,title,artist,path,quality,bytes,status,created_at
             FROM downloads ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(DownloadRow {
                task_id: r.get(0)?,
                provider: r.get(1)?,
                id: r.get(2)?,
                title: r.get(3)?,
                artist: r.get(4)?,
                path: r.get(5)?,
                quality: r.get(6)?,
                bytes: r.get(7)?,
                status: r.get(8)?,
                created_at: r.get(9)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 找一首歌已经下载完成的**本地**文件（离线播放用）。
    ///
    /// 网盘下载（`path` 是 `s3://`）不算：那种还得走签名直链。
    /// 同一个曲目可能下载过多次（不同音质），取最近一条。
    pub fn find_local_download(&self, provider: &str, id: &str) -> Result<Option<DownloadRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT task_id,provider,id,title,artist,path,quality,bytes,status,created_at
             FROM downloads
             WHERE provider=?1 AND id=?2 AND status='done' AND path NOT LIKE 's3://%'
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![provider, id])?;
        let row = match rows.next()? {
            Some(r) => DownloadRow {
                task_id: r.get(0)?,
                provider: r.get(1)?,
                id: r.get(2)?,
                title: r.get(3)?,
                artist: r.get(4)?,
                path: r.get(5)?,
                quality: r.get(6)?,
                bytes: r.get(7)?,
                status: r.get(8)?,
                created_at: r.get(9)?,
            },
            None => return Ok(None),
        };
        Ok(Some(row))
    }

    // ---------- 会话 ----------

    pub fn save_session(&self, s: &Session) -> Result<()> {
        let cookies = serde_json::to_string(&s.cookies).unwrap_or_else(|_| "[]".into());
        let extra = serde_json::to_string(&s.extra).unwrap_or_else(|_| "{}".into());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sessions
             (provider,user_id,nickname,avatar,cookies,extra,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                s.provider,
                s.user_id,
                s.nickname,
                s.avatar,
                cookies,
                extra,
                s.created_at
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, provider: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT provider,user_id,nickname,avatar,cookies,extra,created_at
             FROM sessions WHERE provider=?1",
        )?;
        let mut rows = stmt.query(params![provider])?;
        if let Some(row) = rows.next()? {
            let cookies: String = row.get(4)?;
            let extra: String = row.get(5)?;
            return Ok(Some(Session {
                provider: row.get(0)?,
                user_id: row.get(1)?,
                nickname: row.get(2)?,
                avatar: row.get(3)?,
                cookies: serde_json::from_str(&cookies).unwrap_or_default(),
                extra: serde_json::from_str(&extra).unwrap_or_default(),
                created_at: row.get(6)?,
            }));
        }
        Ok(None)
    }

    pub fn delete_session(&self, provider: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE provider=?1", params![provider])?;
        Ok(())
    }

    // ---------- 收藏歌单 ----------

    pub fn save_playlist(&self, p: &PlaylistRef) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO saved_playlists
             (provider,playlist_id,name,cover,track_count,creator,added_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                p.provider,
                p.id,
                p.name,
                p.cover,
                p.track_count as i64,
                p.creator,
                now_ts()
            ],
        )?;
        Ok(())
    }

    pub fn remove_playlist(&self, provider: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM saved_playlists WHERE provider=?1 AND playlist_id=?2",
            params![provider, id],
        )?;
        Ok(())
    }

    pub fn list_saved_playlists(&self) -> Result<Vec<PlaylistRef>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT provider,playlist_id,name,cover,track_count,creator
             FROM saved_playlists ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PlaylistRef {
                provider: r.get(0)?,
                id: r.get(1)?,
                name: r.get(2)?,
                cover: r.get(3)?,
                track_count: r.get::<_, i64>(4).unwrap_or(0) as u32,
                creator: r.get(5)?,
                description: None,
                play_count: 0,
                tags: None,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ---------- 网盘歌单 ----------

    pub fn save_s3_playlist(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO s3_playlists (name, created_at, sort_order)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM s3_playlists))",
            params![name, now_ts()],
        )?;
        Ok(())
    }

    pub fn list_s3_playlists(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT name FROM s3_playlists ORDER BY sort_order ASC, created_at ASC")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 按前端拖拽后的顺序重排歌单
    pub fn reorder_s3_playlists(&self, names: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (i, name) in names.iter().enumerate() {
            tx.execute(
                "UPDATE s3_playlists SET sort_order=?1 WHERE name=?2",
                params![i as i64, name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_s3_playlist(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM s3_playlists WHERE name=?1", params![name])?;
        Ok(())
    }

    pub fn is_saved_playlist(&self, provider: &str, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM saved_playlists WHERE provider=?1 AND playlist_id=?2",
            params![provider, id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Track {
        Track {
            provider: "kuwo".into(),
            id: "123".into(),
            title: "稻香".into(),
            artist: "周杰伦".into(),
            artist_id: "336".into(),
            album: "魔杰座".into(),
            album_id: "29218".into(),
            duration_sec: 223,
            cover: Some("https://img1.kuwo.cn/star/albumcover/x.jpg".into()),
            availability: crate::model::Availability::Unknown,
            subtitle: None,
            hot: 0,
            available_qualities: vec![
                crate::model::Quality::Standard,
                crate::model::Quality::Super,
                crate::model::Quality::Lossless,
            ],
            has_advanced: true,
        }
    }

    #[test]
    fn finds_completed_local_download_only() {
        let s = Storage::open_in_memory().unwrap();
        let t = sample();
        // 只有「本地 + 完成」的记录才算，网盘路径和失败记录都不算
        s.insert_download("a", &t, crate::model::Quality::Super, "/tmp/x.mp3", 100, "running")
            .unwrap();
        assert!(s.find_local_download("kuwo", "123").unwrap().is_none());

        s.update_download("a", "done", 100).unwrap();
        let row = s.find_local_download("kuwo", "123").unwrap().expect("应该找到本地文件");
        assert_eq!(row.path, "/tmp/x.mp3");
        assert_eq!(row.quality, "super");

        s.insert_download("b", &t, crate::model::Quality::Lossless, "s3://bucket/x.flac", 1, "done")
            .unwrap();
        let row = s.find_local_download("kuwo", "123").unwrap().expect("还是本地那条");
        assert_eq!(row.path, "/tmp/x.mp3");
    }

    #[test]
    fn favorites_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let t = sample();
        s.add_favorite(&t).unwrap();
        assert!(s.is_favorite("kuwo", "123").unwrap());
        let list = s.list_favorites().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "稻香");
        assert!(list[0]
            .available_qualities
            .contains(&crate::model::Quality::Lossless));
        assert!(list[0].has_advanced);
        s.remove_favorite("kuwo", "123").unwrap();
        assert!(!s.is_favorite("kuwo", "123").unwrap());
        assert!(s.list_favorites().unwrap().is_empty());
    }

    #[test]
    fn downloads_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let t = sample();
        s.insert_download("task1", &t, Quality::Standard, "/tmp/a.mp3", 0, "running")
            .unwrap();
        s.update_download("task1", "done", 3182050).unwrap();
        let rows = s.list_downloads().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "done");
        assert_eq!(rows[0].bytes, 3182050);
    }

    #[test]
    fn session_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        assert!(s.get_session("kuwo").unwrap().is_none());

        let mut extra = std::collections::HashMap::new();
        extra.insert("loginSid".to_string(), "abc".to_string());
        let sess = Session {
            provider: "kuwo".into(),
            user_id: "42".into(),
            nickname: "小明".into(),
            avatar: None,
            cookies: vec![("kw_token".into(), "xyz".into())],
            extra,
            created_at: 1,
        };
        s.save_session(&sess).unwrap();

        let got = s.get_session("kuwo").unwrap().unwrap();
        assert_eq!(got.nickname, "小明");
        assert_eq!(got.cookies.len(), 1);
        assert_eq!(got.cookies[0].0, "kw_token");
        assert_eq!(got.extra.get("loginSid").unwrap(), "abc");
        assert_eq!(got.cookie_header(), "kw_token=xyz");

        s.delete_session("kuwo").unwrap();
        assert!(s.get_session("kuwo").unwrap().is_none());
    }

    #[test]
    fn saved_playlists_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let p = PlaylistRef {
            provider: "kuwo".into(),
            id: "2867496601".into(),
            name: "周杰伦精选".into(),
            cover: Some("https://img1.kuwo.cn/x.jpg".into()),
            track_count: 177,
            creator: Some("someone".into()),
            description: None,
            play_count: 0,
            tags: None,
        };
        assert!(!s.is_saved_playlist("kuwo", "2867496601").unwrap());
        s.save_playlist(&p).unwrap();
        assert!(s.is_saved_playlist("kuwo", "2867496601").unwrap());
        let list = s.list_saved_playlists().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].track_count, 177);
        // 重复保存不产生重复行
        s.save_playlist(&p).unwrap();
        assert_eq!(s.list_saved_playlists().unwrap().len(), 1);
        s.remove_playlist("kuwo", "2867496601").unwrap();
        assert!(s.list_saved_playlists().unwrap().is_empty());
    }

    #[test]
    fn s3_playlists_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        s.save_s3_playlist("周杰伦").unwrap();
        s.save_s3_playlist("通勤").unwrap();
        s.save_s3_playlist("周杰伦").unwrap(); // 幂等
        assert_eq!(s.list_s3_playlists().unwrap(), vec!["周杰伦", "通勤"]);
        s.remove_s3_playlist("周杰伦").unwrap();
        assert_eq!(s.list_s3_playlists().unwrap(), vec!["通勤"]);
    }

    #[test]
    fn s3_playlists_reorder() {
        let s = Storage::open_in_memory().unwrap();
        for n in ["A", "B", "C"] {
            s.save_s3_playlist(n).unwrap();
        }
        s.reorder_s3_playlists(&["C".into(), "A".into(), "B".into()])
            .unwrap();
        assert_eq!(s.list_s3_playlists().unwrap(), vec!["C", "A", "B"]);
        s.save_s3_playlist("D").unwrap();
        assert_eq!(s.list_s3_playlists().unwrap(), vec!["C", "A", "B", "D"]);
    }

    #[test]
    fn upsert_is_idempotent() {
        let s = Storage::open_in_memory().unwrap();
        let mut t = sample();
        s.upsert_track(&t).unwrap();
        t.title = "新标题".into();
        s.upsert_track(&t).unwrap();
        s.add_favorite(&t).unwrap();
        let list = s.list_favorites().unwrap();
        assert_eq!(list[0].title, "新标题");
    }

    #[test]
    fn s3_sources_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let t = sample();
        s.insert_download(
            "x",
            &t,
            Quality::Super,
            "s3://bucket/TinyMusic/喜欢/周杰伦 - 稻香.mp3",
            10,
            "done",
        )
        .unwrap();
        let rows = s.list_s3_sources().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "s3://bucket/TinyMusic/喜欢/周杰伦 - 稻香.mp3");
        assert_eq!(rows[0].1.title, "稻香");
        assert_eq!(rows[0].1.cover, t.cover, "完整曲目信息要能还原");
        // 本地下载记录不该混进来
        s.insert_download("y", &t, Quality::Super, "/tmp/a.mp3", 10, "done")
            .unwrap();
        assert_eq!(s.list_s3_sources().unwrap().len(), 1);
    }

    #[test]
    fn download_failures_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let mut f = FailedDownload {
            id: "local:kuwo:123".into(),
            track: sample(),
            quality: "super".into(),
            target: "local".into(),
            playlist: String::new(),
            location: "/tmp/x.mp3".into(),
            parts: vec![FailedPart {
                part: "cover".into(),
                error: "封面 HTTP 403".into(),
            }],
            created_at: 0,
        };
        s.upsert_download_failure(&f).unwrap();
        let list = s.list_download_failures().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].track.title, "稻香");
        assert_eq!(list[0].parts[0].part, "cover");
        assert!(list[0].created_at > 0);

        // 同 id 覆盖，不产生第二条
        f.parts = vec![
            FailedPart {
                part: "audio".into(),
                error: "请求超时".into(),
            },
            FailedPart {
                part: "lyrics".into(),
                error: "请求超时".into(),
            },
        ];
        f.location = String::new();
        s.upsert_download_failure(&f).unwrap();
        let list = s.list_download_failures().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].parts.len(), 2);

        s.remove_download_failure("local:kuwo:123").unwrap();
        assert!(s.list_download_failures().unwrap().is_empty());
        s.upsert_download_failure(&f).unwrap();
        s.clear_download_failures().unwrap();
        assert!(s.list_download_failures().unwrap().is_empty());
    }
}
