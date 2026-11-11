//! 网易云音乐 provider。
//!
//! 目标很具体：**用网易云的登录态与歌单/推荐接口，给酷我补曲库**。
//! 因此这个 provider 有两个职责：
//!
//! 1. 目录侧：登录、每日推荐、推荐歌单、歌单详情、搜索、歌词、专辑、艺人；
//! 2. 播放侧：先试网易云直链（免费曲成功率不错），不行就按
//!    「标题 + 歌手 + 时长」到酷我匹配同一首歌（`bridge`）。
//!
//! ## 实测数据（2026-11-10，匿名，热歌榜前 20 首）
//!
//! ```text
//! 网易云匿名取流 320k：12/20 成功（code=-110 的 8 首全是 fee=1 会员曲）
//! 酷我按「标题+歌手」直接取：只有 2/20 命中，且多为翻唱 —— 排序不可靠
//! ```
//!
//! 所以「网易云拿目录 + 酷我拿音源」这条路能走通，但**命中率取决于匹配质量**，
//! 不是「接口调通就能播」：必须做标题规范化 + 歌手校验 + 时长校验，
//! 低置信度宁可报不可用。

pub mod bridge;
pub mod weapi;

use crate::cache::TtlCache;
use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::model::*;
use crate::quality;
use crate::text::clean;
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 网易云图片统一缩放：`?param=WxH`
///
/// 接口里的 `picUrl` / `coverImgUrl` 大量还是 `http://`（每日推荐、我的歌单尤其明显），
/// 而应用页面跑在 Tauri 的自定义协议上，WebView 会把这种图片当混合内容拦掉 ——
/// 表现就是一张都加载不出来。这里统一升到 https：`p*.music.126.net` 两种都支持。
pub fn cover_url(raw: &str, size: u32) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let base = raw.split('?').next().unwrap_or(raw);
    let base = match base.strip_prefix("http://") {
        Some(rest) => format!("https://{}", rest),
        None => base.to_string(),
    };
    Some(format!("{}?param={}y{}", base, size, size))
}

/// 播放策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// 先试网易云直链，失败再按标题/歌手/时长去酷我找（默认）
    NeteaseFirst,
    /// 只用酷我音源（把网易云当纯目录）
    KuwoOnly,
}

impl PlayMode {
    pub fn parse(s: &str) -> PlayMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "kuwo_only" | "kuwo" => PlayMode::KuwoOnly,
            _ => PlayMode::NeteaseFirst,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PlayMode::NeteaseFirst => "netease_first",
            PlayMode::KuwoOnly => "kuwo_only",
        }
    }
    fn key(self) -> &'static str {
        match self {
            PlayMode::NeteaseFirst => "ne",
            PlayMode::KuwoOnly => "kw",
        }
    }
}

pub struct NeteaseProvider {
    http: Arc<HttpClient>,
    api: weapi::WeapiClient,
    search_cache: TtlCache<String, SearchPage>,
    playlist_cache: TtlCache<String, PlaylistDetail>,
    album_cache: TtlCache<String, AlbumPage>,
    artist_cache: TtlCache<String, ArtistPage>,
    detail_cache: TtlCache<String, TrackDetail>,
    resolve_cache: TtlCache<String, StreamInfo>,
    recommend_cache: TtlCache<String, Vec<Track>>,
    /// 跨源兜底：酷我
    fallback: Option<Arc<dyn MusicProvider>>,
    /// 播放策略（`PlayMode` 的原子存储：0 = 网易云优先，1 = 只用酷我）
    play_mode: AtomicU8,
}

impl Default for NeteaseProvider {
    fn default() -> Self {
        Self::new(None)
    }
}

impl NeteaseProvider {
    pub fn new(fallback: Option<Arc<dyn MusicProvider>>) -> Self {
        Self::with_http(Arc::new(HttpClient::new(150, 3)), fallback)
    }

    pub fn with_http(http: Arc<HttpClient>, fallback: Option<Arc<dyn MusicProvider>>) -> Self {
        Self {
            api: weapi::WeapiClient::new(http.clone()),
            http,
            search_cache: TtlCache::new(Duration::from_secs(600), 128),
            playlist_cache: TtlCache::new(Duration::from_secs(30 * 60), 64),
            album_cache: TtlCache::new(Duration::from_secs(6 * 3600), 64),
            artist_cache: TtlCache::new(Duration::from_secs(6 * 3600), 64),
            detail_cache: TtlCache::new(Duration::from_secs(6 * 3600), 128),
            resolve_cache: TtlCache::new(Duration::from_secs(15 * 60), 256),
            recommend_cache: TtlCache::new(Duration::from_secs(30 * 60), 8),
            fallback,
            play_mode: AtomicU8::new(0),
        }
    }

    pub fn set_play_mode(&self, mode: PlayMode) {
        self.play_mode.store(
            match mode {
                PlayMode::NeteaseFirst => 0,
                PlayMode::KuwoOnly => 1,
            },
            Ordering::Relaxed,
        );
    }

    pub fn play_mode(&self) -> PlayMode {
        if self.play_mode.load(Ordering::Relaxed) == 1 {
            PlayMode::KuwoOnly
        } else {
            PlayMode::NeteaseFirst
        }
    }

    /// 是否已持有登录票据（目前看 `MUSIC_U`）
    pub async fn logged_in(&self) -> bool {
        self.api.has_login()
    }

    pub async fn cookie_header(&self) -> String {
        self.api.cookie_header()
    }

    // ---------- 字段映射 ----------

    /// 网易云的 `fee`：0 免费 / 8 免费（低音质优先）/ 1 会员 / 4 付费专辑
    ///
    /// 实测：热歌榜里 `fee=8` 的曲目照样能拿到 320k 直链，
    /// 真正取不到的是 `fee=1`（`code=-110`）。所以 8 不能标成「仅试听」。
    fn availability_of(fee: i64) -> Availability {
        match fee {
            0 | 8 => Availability::Available,
            1 | 4 => Availability::VipOnly,
            _ => Availability::Unknown,
        }
    }

    /// 同时兼容 `cloudsearch`（`ar`/`al`/`dt`）与旧 `search/get`（`artists`/`album`/`duration`）
    pub fn map_song(v: &Value) -> Option<Track> {
        let id = v.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
        if id == 0 {
            return None;
        }
        let title = clean(v.get("name").and_then(|x| x.as_str()).unwrap_or(""));
        if title.is_empty() {
            return None;
        }

        let artists = v
            .get("ar")
            .or_else(|| v.get("artists"))
            .and_then(|x| x.as_array());
        let artist = artists
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                    .map(clean)
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        let artist_id = artists
            .and_then(|arr| arr.first())
            .and_then(|a| a.get("id"))
            .and_then(|x| x.as_i64())
            .map(|x| x.to_string())
            .unwrap_or_default();

        let album = v.get("al").or_else(|| v.get("album"));
        let album_name = album
            .and_then(|a| a.get("name"))
            .and_then(|x| x.as_str())
            .map(clean)
            .unwrap_or_default();
        let album_id = album
            .and_then(|a| a.get("id"))
            .and_then(|x| x.as_i64())
            .map(|x| x.to_string())
            .unwrap_or_default();
        let cover = album
            .and_then(|a| a.get("picUrl"))
            .and_then(|x| x.as_str())
            .and_then(|u| cover_url(u, 300));

        let dur_ms = v
            .get("dt")
            .or_else(|| v.get("duration"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);

        let fee = v.get("fee").and_then(|x| x.as_i64()).unwrap_or(0);

        let subtitle = v
            .get("alias")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| clean(s));

        Some(Track {
            provider: "netease".into(),
            id: id.to_string(),
            title,
            artist,
            artist_id,
            album: album_name,
            album_id,
            duration_sec: (dur_ms / 1000).max(0) as u32,
            cover,
            availability: Self::availability_of(fee),
            subtitle,
            hot: 0,
            available_qualities: Vec::new(),
            has_advanced: false,
        })
    }

    pub fn map_playlist(v: &Value) -> Option<PlaylistRef> {
        let id = v.get("id").and_then(|x| x.as_i64()).unwrap_or(0);
        if id == 0 {
            return None;
        }
        let cover = v
            .get("coverImgUrl")
            .or_else(|| v.get("picUrl"))
            .and_then(|x| x.as_str())
            .and_then(|u| cover_url(u, 400));
        let creator = v
            .get("creator")
            .and_then(|c| c.get("nickname"))
            .and_then(|x| x.as_str())
            .or_else(|| v.get("creator").and_then(|c| c.as_str()))
            .map(clean);
        let desc = v
            .get("description")
            .and_then(|x| x.as_str())
            .map(clean)
            .filter(|s| !s.is_empty());
        let tags = v
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .filter(|s| !s.is_empty())
            .or_else(|| v.get("tag").and_then(|x| x.as_str()).map(|s| clean(s)));

        Some(PlaylistRef {
            provider: "netease".into(),
            id: id.to_string(),
            name: clean(v.get("name").and_then(|x| x.as_str()).unwrap_or("未命名歌单")),
            cover,
            track_count: v
                .get("trackCount")
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32,
            creator,
            description: desc,
            play_count: v
                .get("playCount")
                .or_else(|| v.get("playcount"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            tags,
        })
    }

    // ---------- 接口封装 ----------

    /// 批量取歌曲详情（`song/detail` 免登录，最多一次 500 个 id）
    async fn songs_by_ids(&self, ids: &[i64]) -> Result<Vec<Track>> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let joined = format!(
                "[{}]",
                chunk
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let v = self
                .api
                .post("song/detail", json!({ "ids": joined }))
                .await?;
            if let Some(songs) = v.get("songs").and_then(|s| s.as_array()) {
                out.extend(songs.iter().filter_map(Self::map_song));
            }
        }
        Ok(out)
    }

    /// 当前账号的「每日推荐」。
    ///
    /// 实测（2026-11-10，带 MUSIC_U）：
    /// * `v3/discovery/recommend/songs` → 200，`data.dailySongs` 33 首，带 `ar`/`al`/`dt`
    /// * `v2/discovery/recommend/songs` → 200，同样 33 首，但曲目字段是老结构（`artists`/`album`）
    /// * 两者都要求登录；匿名时 v2 返回 `code=301`（v3 会给一份不含个人化的列表）
    ///
    /// 所以先走 v3，失败再退回 v2。
    async fn daily_songs(&self, limit: u32) -> Result<Vec<Track>> {
        let payload = json!({
            "limit": limit.clamp(1, 100).to_string(),
            "offset": "0",
            "total": "true",
        });

        let mut last_err: Option<Error> = None;
        for path in ["v3/discovery/recommend/songs", "v2/discovery/recommend/songs"] {
            match self.api.post(path, payload.clone()).await {
                Ok(v) => {
                    let code = weapi::code_of(&v);
                    if code != 200 {
                        last_err = Some(Error::Unsupported(format!(
                            "{}（code={code}）",
                            weapi::explain_code(code)
                        )));
                        continue;
                    }
                    let songs = v
                        .get("data")
                        .and_then(|d| d.get("dailySongs"))
                        .and_then(|s| s.as_array())
                        .map(|arr| {
                            arr.iter().filter_map(Self::map_song).collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if !songs.is_empty() {
                        return Ok(songs);
                    }
                    last_err = Some(Error::Unsupported(
                        "每日推荐为空（账号当天还没有生成）".into(),
                    ));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| Error::Unsupported("每日推荐不可用".into())))
    }

    /// 取流：优先网易云直链
    async fn netease_stream(&self, track: &Track, q: Quality) -> Result<StreamInfo> {
        if q.is_advanced() {
            return Err(Error::Unsupported(format!(
                "{} 需要会员加密音频解密，未接入",
                q.label()
            )));
        }
        let level = match q {
            Quality::Lossless => "lossless",
            Quality::Super => "exhigh",
            _ => "standard",
        };
        let encode = if q == Quality::Lossless { "flac" } else { "mp3" };
        let v = self
            .api
            .post(
                "song/enhance/player/url/v1",
                json!({
                    "ids": format!("[{}]", track.id),
                    "level": level,
                    "encodeType": encode,
                }),
            )
            .await?;

        let item = v
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        let code = item.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let url = item
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();

        if code != 200 || url.is_empty() {
            return Err(Error::Unsupported(format!(
                "网易云直链不可用（code={code}，{}）",
                weapi::explain_code(code)
            )));
        }

        // 免费曲/会员曲都得校验真实文件：网易云同样会下发短片段
        let mut info = quality::verify_url(&self.http, &url, track.duration_sec, q, "netease").await?;
        // 网易云自己报的码率更准，回填一次
        if let Some(br) = item.get("br").and_then(|b| b.as_u64()) {
            if br > 0 {
                info.format.bitrate_kbps = Some((br / 1000) as u32);
                if let Some(tier) = info.format.tier() {
                    info.degraded = tier.rank() < q.rank();
                    info.quality = if info.format.lossless { Quality::Lossless } else { tier };
                }
            }
        }
        Ok(info)
    }

    /// 取流：按「标题 + 歌手 + 时长」到酷我找同一首歌
    async fn kuwo_stream(&self, track: &Track, q: Quality) -> Result<StreamInfo> {
        let Some(kw) = self.fallback.as_ref() else {
            return Err(Error::Unsupported("未接入酷我，无法跨源播放".into()));
        };
        let Some(m) = bridge::find_in_kuwo(kw.as_ref(), track).await else {
            return Err(Error::Unavailable);
        };
        let mut info = kw.resolve(&m.track, q).await.map_err(|e| {
            Error::Http(format!(
                "酷我命中「{}」但取流失败：{e}",
                m.track.title
            ))
        })?;
        info.via = format!("kuwo-bridge/{:.2}", m.score);
        info.availability = Availability::Available;
        Ok(info)
    }

    /// 拉起登录态：cookie 是否真的有效由 `w/nuser/account/get` 决定
    async fn account(&self) -> Result<Value> {
        let v = self.api.post("w/nuser/account/get", json!({})).await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        Ok(v)
    }

    fn session_from_account(&self, v: &Value, cookie: String) -> Result<Session> {
        let profile = v.get("profile").filter(|p| !p.is_null());
        let Some(profile) = profile else {
            return Err(Error::Unsupported(
                "cookie 无效或已过期（网易云没有返回账号信息）".into(),
            ));
        };
        let cookies = cookie
            .split(';')
            .filter_map(|p| p.trim().split_once('='))
            .map(|(k, val)| (k.trim().to_string(), val.trim().to_string()))
            .filter(|(k, _)| !k.is_empty())
            .collect::<Vec<_>>();

        let mut extra = std::collections::HashMap::new();
        if let Some(vip) = profile.get("vipType").and_then(|x| x.as_i64()) {
            extra.insert("vip_type".into(), vip.to_string());
        }
        if let Some(t) = profile.get("createTime").and_then(|x| x.as_i64()) {
            extra.insert("created_time".into(), t.to_string());
        }

        Ok(Session {
            provider: "netease".into(),
            user_id: profile
                .get("userId")
                .and_then(|x| x.as_i64())
                .map(|x| x.to_string())
                .unwrap_or_default(),
            nickname: clean(
                profile
                    .get("nickname")
                    .and_then(|x| x.as_str())
                    .unwrap_or(""),
            ),
            avatar: profile
                .get("avatarUrl")
                .and_then(|x| x.as_str())
                .and_then(|u| cover_url(u, 200)),
            cookies,
            extra,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        })
    }

    /// 账号歌单（含「喜欢的音乐」，`specialType=5`）
    async fn playlists_of(&self, uid: &str) -> Result<Vec<PlaylistRef>> {
        let v = self
            .api
            .post(
                "user/playlist",
                json!({ "uid": uid, "limit": "1000", "offset": "0", "includeVideo": "true" }),
            )
            .await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        Ok(v.get("playlist")
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(Self::map_playlist).collect())
            .unwrap_or_default())
    }
}

#[async_trait::async_trait]
impl MusicProvider for NeteaseProvider {
    fn id(&self) -> &'static str {
        "netease"
    }

    fn display_name(&self) -> &'static str {
        "网易云音乐"
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "netease".into(),
            name: "网易云音乐".into(),
            qualities: vec![Quality::Standard, Quality::Super, Quality::Lossless],
        }
    }

    fn features(&self) -> ProviderFeatures {
        ProviderFeatures {
            favorites: true,
            recommend: true,
        }
    }

    fn auth_capabilities(&self) -> Vec<AuthCapability> {
        vec![
            AuthCapability::ok(AuthMethod::Cookie),
            AuthCapability {
                method: AuthMethod::Password,
                supported: true,
                reason: Some(
                    "手机号+密码可用；若账号开了设备/短信校验，会返回 8821，此时请用网页登录或 cookie"
                        .into(),
                ),
            },
            AuthCapability::blocked(
                AuthMethod::QrCode,
                "扫码需要在界面里渲染二维码，暂未接入；请用「网页登录」（底层同为 cookie）",
            ),
        ]
    }

    fn set_session(&self, session: &Session) {
        let raw = session.cookie_header();
        if !raw.trim().is_empty() {
            self.api.set_cookie_header(&raw);
        }
    }

    async fn search(&self, kw: &str, page: u32, limit: u32) -> Result<SearchPage> {
        let key = format!("{}|{}|{}", kw, page, limit);
        if let Some(hit) = self.search_cache.get(&key) {
            return Ok(hit);
        }
        let limit = limit.clamp(1, 100);
        let offset = page.saturating_mul(limit);
        let v = self
            .api
            .post(
                "cloudsearch/pc",
                json!({
                    "s": kw,
                    "type": "1",
                    "limit": limit.to_string(),
                    "offset": offset.to_string(),
                    "total": "true",
                }),
            )
            .await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        let result = v.get("result").cloned().unwrap_or(Value::Null);
        let items = result
            .get("songs")
            .and_then(|s| s.as_array())
            .map(|arr| arr.iter().filter_map(Self::map_song).collect::<Vec<_>>())
            .unwrap_or_default();
        let out = SearchPage {
            total: result
                .get("songCount")
                .and_then(|x| x.as_u64())
                .unwrap_or(items.len() as u64) as u32,
            page,
            limit,
            items,
        };
        self.search_cache.put(key, out.clone());
        Ok(out)
    }

    async fn resolve(&self, track: &Track, q: Quality) -> Result<StreamInfo> {
        let mode = self.play_mode();
        let key = format!("{}|{}|{}", track.id, q.as_str(), mode.key());
        if let Some(hit) = self.resolve_cache.get(&key) {
            return Ok(hit);
        }

        // 1) 网易云直链（KuwoOnly 时跳过）
        if mode == PlayMode::NeteaseFirst {
            // 已知是会员曲且没登录：直接走桥接，省一次必然失败的请求
            let logged = self.logged_in().await;
            if logged || track.availability != Availability::VipOnly {
                if let Ok(info) = self.netease_stream(track, q).await {
                    self.resolve_cache.put(key, info.clone());
                    return Ok(info);
                }
            }
        }

        // 2) 酷我跨源匹配
        match self.kuwo_stream(track, q).await {
            Ok(info) => {
                self.resolve_cache.put(key, info.clone());
                Ok(info)
            }
            Err(e) => {
                let why = if mode == PlayMode::KuwoOnly {
                    "酷我未匹配到同一首歌".to_string()
                } else if self.logged_in().await {
                    format!("网易云直链与酷我兜底都失败（{e}）")
                } else {
                    format!("未登录网易云且酷我未匹配到（{e}）")
                };
                Err(Error::Unsupported(why))
            }
        }
    }

    async fn track_qualities(&self, track: &Track) -> Result<TrackQualities> {
        // 逐档问一次，能拿到直链的档位才算「可播」
        let mut available = Vec::new();
        for (level, q) in [
            ("standard", Quality::Standard),
            ("exhigh", Quality::Super),
            ("lossless", Quality::Lossless),
        ] {
            let encode = if q == Quality::Lossless { "flac" } else { "mp3" };
            let v = self
                .api
                .post(
                    "song/enhance/player/url/v1",
                    json!({ "ids": format!("[{}]", track.id), "level": level, "encodeType": encode }),
                )
                .await;
            let Ok(v) = v else { continue };
            let ok = v
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|a| a.first())
                .map(|x| {
                    x.get("code").and_then(|c| c.as_i64()) == Some(200)
                        && x.get("url")
                            .and_then(|u| u.as_str())
                            .map(|u| !u.is_empty())
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if ok {
                available.push(q);
            }
        }
        Ok(TrackQualities {
            available,
            has_advanced: false,
            known: true,
        })
    }

    fn invalidate_resolve(&self, id: &str) {
        // 键里带质量和模式，直接整表清掉最省事（条目本来就少）
        let _ = id;
        self.resolve_cache.clear();
    }

    async fn detail(&self, track: &Track) -> Result<TrackDetail> {
        if let Some(hit) = self.detail_cache.get(&track.id) {
            return Ok(hit);
        }
        let v = self
            .api
            .post(
                "song/lyric",
                json!({ "id": track.id, "lv": "1", "kv": "1", "tv": "-1" }),
            )
            .await?;

        let mut lyrics = v
            .get("lrc")
            .and_then(|l| l.get("lyric"))
            .and_then(|x| x.as_str())
            .map(crate::text::parse_lrc)
            .unwrap_or_default();

        // 翻译歌词：同时间戳的行合并成「原文 / 译文」
        if let Some(t) = v
            .get("tlyric")
            .and_then(|l| l.get("lyric"))
            .and_then(|x| x.as_str())
        {
            let trans = crate::text::parse_lrc(t);
            for line in lyrics.iter_mut() {
                if let Some(tr) = trans.iter().find(|x| x.time_ms == line.time_ms) {
                    if !tr.text.trim().is_empty() && tr.text != line.text {
                        line.text = format!("{} / {}", line.text, tr.text);
                    }
                }
            }
        }

        let out = TrackDetail {
            track: track.clone(),
            lyrics,
            preview_range_ms: None,
        };
        self.detail_cache.put(track.id.clone(), out.clone());
        Ok(out)
    }

    async fn artist(&self, artist_id: &str, name: &str, _page: u32) -> Result<ArtistPage> {
        if let Some(hit) = self.artist_cache.get(&artist_id.to_string()) {
            return Ok(hit);
        }
        let v = self.api.post(&format!("v1/artist/{artist_id}"), json!({})).await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        let artist = v.get("artist").cloned().unwrap_or(Value::Null);
        let tracks = v
            .get("hotSongs")
            .and_then(|s| s.as_array())
            .map(|arr| arr.iter().filter_map(Self::map_song).collect::<Vec<_>>())
            .unwrap_or_default();

        let pic = artist
            .get("picUrl")
            .or_else(|| artist.get("img1v1Url"))
            .and_then(|x| x.as_str())
            .and_then(|u| cover_url(u, 400));

        // 专辑列表是加分项：失败不影响页面（`artist/album` 已废弃，走 `artist/albums/{id}`）
        let albums = match self
            .api
            .post(
                &format!("artist/albums/{artist_id}"),
                json!({ "limit": "50", "offset": "0", "total": "true" }),
            )
            .await
        {
            Ok(a) => a
                .get("hotAlbums")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|al| {
                            let id = al.get("id").and_then(|x| x.as_i64())?;
                            Some(AlbumRef {
                                id: id.to_string(),
                                name: clean(al.get("name").and_then(|x| x.as_str()).unwrap_or("")),
                                cover: al
                                    .get("picUrl")
                                    .and_then(|x| x.as_str())
                                    .and_then(|u| cover_url(u, 300)),
                                year: al
                                    .get("publishTime")
                                    .and_then(|x| x.as_i64())
                                    .map(|ms| (ms / 31_536_000_000 + 1970).to_string()),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let out = ArtistPage {
            artist_id: artist_id.to_string(),
            name: clean(
                artist
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or(name),
            ),
            pic,
            desc: artist
                .get("briefDesc")
                .and_then(|x| x.as_str())
                .map(clean)
                .filter(|s| !s.is_empty()),
            meta: Vec::new(),
            albums,
            tracks,
        };
        self.artist_cache.put(artist_id.to_string(), out.clone());
        Ok(out)
    }

    async fn album(&self, album_id: &str, name: &str, _page: u32) -> Result<AlbumPage> {
        if let Some(hit) = self.album_cache.get(&album_id.to_string()) {
            return Ok(hit);
        }
        let v = self.api.post(&format!("v1/album/{album_id}"), json!({})).await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        let album = v.get("album").cloned().unwrap_or(Value::Null);
        let tracks = v
            .get("songs")
            .and_then(|s| s.as_array())
            .map(|arr| arr.iter().filter_map(Self::map_song).collect::<Vec<_>>())
            .unwrap_or_default();
        let out = AlbumPage {
            album_id: album_id.to_string(),
            name: clean(album.get("name").and_then(|x| x.as_str()).unwrap_or(name)),
            artist: clean(
                album
                    .get("artist")
                    .and_then(|a| a.get("name"))
                    .and_then(|x| x.as_str())
                    .unwrap_or(""),
            ),
            cover: album
                .get("picUrl")
                .and_then(|x| x.as_str())
                .and_then(|u| cover_url(u, 500)),
            tracks,
        };
        self.album_cache.put(album_id.to_string(), out.clone());
        Ok(out)
    }

    async fn search_playlists(&self, kw: &str, page: u32, limit: u32) -> Result<PlaylistPage> {
        let limit = limit.clamp(1, 100);
        let offset = page.saturating_mul(limit);
        let v = self
            .api
            .post(
                "cloudsearch/pc",
                json!({
                    "s": kw,
                    "type": "1000",
                    "limit": limit.to_string(),
                    "offset": offset.to_string(),
                    "total": "true",
                }),
            )
            .await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        let result = v.get("result").cloned().unwrap_or(Value::Null);
        Ok(PlaylistPage {
            total: result
                .get("playlistCount")
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u32,
            page,
            items: result
                .get("playlists")
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().filter_map(Self::map_playlist).collect())
                .unwrap_or_default(),
        })
    }

    /// 歌单详情：`v6/playlist/detail` 只给 `trackIds`，曲目要再查一次 `song/detail`
    ///
    /// 特例 `id == "daily"`：每日推荐不是一个真歌单，但前端把它当歌单一样点进详情页，
    /// 所以这里合成一个「假歌单」返回，详情页、播放全部、序号都能直接复用。
    async fn playlist(&self, id: &str) -> Result<PlaylistDetail> {
        if id == "daily" {
            let tracks = self.daily_songs(100).await?;
            if tracks.is_empty() {
                return Err(Error::Unsupported(
                    "每日推荐为空（账号当天还没有生成）".into(),
                ));
            }
            let playlist = PlaylistRef {
                provider: "netease".into(),
                id: "daily".into(),
                name: "每日推荐".into(),
                cover: tracks.first().and_then(|t| t.cover.clone()),
                track_count: tracks.len() as u32,
                creator: Some("网易云音乐".into()),
                description: Some("根据你的口味生成，每天 6:00 更新".into()),
                play_count: 0,
                tags: None,
            };
            return Ok(PlaylistDetail { playlist, tracks });
        }
        if let Some(hit) = self.playlist_cache.get(&id.to_string()) {
            return Ok(hit);
        }
        let v = self
            .api
            .post("v6/playlist/detail", json!({ "id": id, "n": "1000", "p": "1" }))
            .await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        let pl = v
            .get("playlist")
            .cloned()
            .ok_or_else(|| Error::NotFound("歌单不存在".into()))?;

        // trackIds 是完整的（可能上千条），tracks 只有前几条
        let ids: Vec<i64> = pl
            .get("trackIds")
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("id").and_then(|x| x.as_i64()))
                    .collect()
            })
            .unwrap_or_default();

        let tracks = if !ids.is_empty() {
            // 上限 2000 首，避免超大歌单打出几十个请求
            let cap = ids.len().min(2000);
            self.songs_by_ids(&ids[..cap]).await?
        } else {
            pl.get("tracks")
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter().filter_map(Self::map_song).collect())
                .unwrap_or_default()
        };

        let out = PlaylistDetail {
            playlist: Self::map_playlist(&pl)
                .ok_or_else(|| Error::Parse("歌单字段缺失".into()))?,
            tracks,
        };
        self.playlist_cache.put(id.to_string(), out.clone());
        Ok(out)
    }

    async fn favorites(&self, uid: &str) -> Result<PlaylistDetail> {
        let list = self.playlists_of(uid).await?;
        // 「喜欢的音乐」是 specialType=5 的隐藏歌单，名字在各语言下不同
        let target = list
            .iter()
            .find(|p| p.name.contains("喜欢"))
            .or_else(|| list.first())
            .ok_or_else(|| Error::NotFound("该账号没有歌单".into()))?;
        self.playlist(&target.id).await
    }

    async fn user_playlists(&self, uid: &str, _limit: u32) -> Result<Vec<PlaylistRef>> {
        self.playlists_of(uid).await
    }

    async fn my_playlists(&self, session: &Session) -> Result<Vec<PlaylistRef>> {
        let uid = if !session.user_id.is_empty() {
            session.user_id.clone()
        } else {
            let v = self.account().await?;
            v.get("profile")
                .and_then(|p| p.get("userId"))
                .and_then(|x| x.as_i64())
                .map(|x| x.to_string())
                .ok_or_else(|| Error::Unsupported("未登录".into()))?
        };
        self.playlists_of(&uid).await
    }

    async fn recommend_songs(&self, limit: u32) -> Result<Vec<Track>> {
        let key = format!("daily|{}", limit);
        if let Some(hit) = self.recommend_cache.get(&key) {
            return Ok(hit);
        }
        let out = self.daily_songs(limit).await?;
        self.recommend_cache.put(key, out.clone());
        Ok(out)
    }

    async fn recommend_playlists(&self) -> Result<Vec<PlaylistRef>> {
        let v = self
            .api
            .post("v1/discovery/recommend/resource", json!({}))
            .await?;
        let code = weapi::code_of(&v);
        if code != 200 {
            return Err(Error::Unsupported(format!(
                "{}（code={code}）",
                weapi::explain_code(code)
            )));
        }
        Ok(v.get("recommend")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(Self::map_playlist).collect())
            .unwrap_or_default())
    }

    async fn login_password(&self, user: &str, pass: &str) -> Result<Session> {
        let phone: String = user.chars().filter(|c| c.is_ascii_digit()).collect();
        if phone.len() < 6 {
            return Err(Error::Unsupported(
                "请输入手机号（国际号码请用 cookie 登录）".into(),
            ));
        }
        if pass.is_empty() {
            return Err(Error::Unsupported("密码不能为空".into()));
        }
        let md5 = format!("{:x}", Md5::digest(pass.as_bytes()));

        let v = self
            .api
            .post(
                "login/cellphone",
                json!({
                    "phone": phone,
                    "countrycode": "86",
                    "password": md5,
                    "rememberLogin": "true",
                    // 未登录时 csrf 为空，服务端会新发一个
                    "csrf_token": "",
                }),
            )
            .await?;

        let code = weapi::code_of(&v);
        if code != 200 {
            let msg = v
                .get("message")
                .or_else(|| v.get("msg"))
                .and_then(|m| m.as_str())
                .unwrap_or("");
            return Err(Error::Unsupported(format!(
                "登录失败：{}（code={code}）{msg}",
                weapi::explain_code(code)
            )));
        }

        let cookie = self.api.cookie_header();
        // 登录响应里通常直接带 profile；没有就再查一次
        if v.get("profile").filter(|p| !p.is_null()).is_some() {
            self.session_from_account(&v, cookie)
        } else {
            let acc = self.account().await?;
            self.session_from_account(&acc, cookie)
        }
    }

    async fn login_cookie(&self, cookie: &str) -> Result<Session> {
        if cookie.trim().is_empty() {
            return Err(Error::Unsupported("cookie 不能为空".into()));
        }
        self.api.set_cookie_header(cookie);
        let acc = self.account().await?;
        if acc.get("profile").filter(|p| !p.is_null()).is_none() {
            self.api.clear_cookies();
            return Err(Error::Unsupported(
                "cookie 无效（缺少 MUSIC_U 或已过期）".into(),
            ));
        }
        self.session_from_account(&acc, self.api.cookie_header())
    }

    async fn logout(&self, _session: &Session) -> Result<()> {
        let _ = self.api.post("logout", json!({})).await;
        self.api.clear_cookies();
        self.resolve_cache.clear();
        self.recommend_cache.clear();
        Ok(())
    }
}

impl NeteaseProvider {
    /// 供外壳调用的「当前鉴权状态」（不发请求，只读本地 cookie 罐）
    pub fn session_hint(&self) -> (bool, String) {
        (self.api.has_login(), self.api.cookie_header())
    }

    /// 只走网易云直链、不做酷我兜底。
    ///
    /// 给「仅网易云音源」场景和 live 测试用：能清晰区分
    /// 「网易云自己给不给」和「桥接有没有救回来」。
    pub async fn resolve_direct(&self, track: &Track, q: Quality) -> Result<StreamInfo> {
        self.netease_stream(track, q).await
    }
}

#[cfg(test)]
mod cover_tests {
    use super::cover_url;

    /// 接口给 http 时必须升到 https，否则 WebView 当混合内容拦掉（一张图都不显示）
    #[test]
    fn upgrades_http_to_https() {
        let u = cover_url("http://p1.music.126.net/bhA-CKyrEnMG8uYVtHquHg==/109951172798206512.jpg", 300)
            .expect("非空地址应该能生成封面");
        assert_eq!(
            u,
            "https://p1.music.126.net/bhA-CKyrEnMG8uYVtHquHg==/109951172798206512.jpg?param=300y300"
        );
    }

    #[test]
    fn keeps_https_and_replaces_query() {
        let u = cover_url("https://p2.music.126.net/a.jpg?param=200y200", 400).unwrap();
        assert_eq!(u, "https://p2.music.126.net/a.jpg?param=400y400");
    }

    #[test]
    fn empty_is_none() {
        assert!(cover_url("  ", 300).is_none());
    }
}
