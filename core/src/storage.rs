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

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS tracks (
  provider TEXT NOT NULL, id TEXT NOT NULL, title TEXT, artist TEXT, artist_id TEXT,
  album TEXT, album_id TEXT, duration INTEGER, cover TEXT,
  PRIMARY KEY (provider, id)
);
CREATE TABLE IF NOT EXISTS downloads (
  task_id TEXT PRIMARY KEY, provider TEXT, id TEXT, title TEXT, artist TEXT,
  path TEXT, quality TEXT, bytes INTEGER, status TEXT, created_at INTEGER
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
"#;

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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_track(&self, t: &Track) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracks (provider,id,title,artist,artist_id,album,album_id,duration,cover)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(provider,id) DO UPDATE SET
               title=excluded.title, artist=excluded.artist, artist_id=excluded.artist_id,
               album=excluded.album, album_id=excluded.album_id,
               duration=excluded.duration, cover=excluded.cover",
            params![
                t.provider, t.id, t.title, t.artist, t.artist_id,
                t.album, t.album_id, t.duration_sec, t.cover
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
            "SELECT t.provider,t.id,t.title,t.artist,t.artist_id,t.album,t.album_id,t.duration,t.cover
             FROM favorites f JOIN tracks t ON t.provider=f.provider AND t.id=f.id
             ORDER BY f.added_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
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
                availability: crate::model::Availability::Unknown,
                subtitle: None,
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO downloads
             (task_id,provider,id,title,artist,path,quality,bytes,status,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
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
                now_ts()
            ],
        )?;
        Ok(())
    }

    pub fn update_download(&self, task_id: &str, status: &str, bytes: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE downloads SET status=?2, bytes=?3 WHERE task_id=?1",
            params![task_id, status, bytes],
        )?;
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
                s.provider, s.user_id, s.nickname, s.avatar, cookies, extra, s.created_at
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
        }
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
        s.remove_favorite("kuwo", "123").unwrap();
        assert!(!s.is_favorite("kuwo", "123").unwrap());
        assert!(s.list_favorites().unwrap().is_empty());
    }

    #[test]
    fn downloads_roundtrip() {
        let s = Storage::open_in_memory().unwrap();
        let t = sample();
        s.insert_download("task1", &t, Quality::High, "/tmp/a.mp3", 0, "running")
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
}
