use serde::{Deserialize, Serialize};

/// 音质档。映射到酷我的 `br` 参数（见 `Quality::br`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// L 档（流畅）
    Standard,
    /// H 档（标准，128k）
    #[default]
    High,
    /// S 档（高品质，320k）
    Super,
    /// F 档（无损）
    Lossless,
}

impl Quality {
    pub fn br(self) -> &'static str {
        match self {
            // 实测：L 档用 128kmp3 亦被服务端接受；96kmp3 为保留值
            Quality::Standard => "128kmp3",
            // 实测 192kmp3 与 128kmp3 返回同一文件，保留以兼容旧服务端
            Quality::High => "128kmp3",
            Quality::Super => "320kmp3",
            Quality::Lossless => "2000kflac",
        }
    }

    pub fn nominal_kbps(self) -> u32 {
        match self {
            Quality::Standard | Quality::High => 128,
            Quality::Super => 320,
            Quality::Lossless => 900,
        }
    }

    /// 降级顺序：无损 → 高品质 → 标准 → 无
    pub fn next_lower(self) -> Option<Quality> {
        match self {
            Quality::Lossless => Some(Quality::Super),
            Quality::Super => Some(Quality::High),
            Quality::High => Some(Quality::Standard),
            Quality::Standard => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Standard => "standard",
            Quality::High => "high",
            Quality::Super => "super",
            Quality::Lossless => "lossless",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Quality {
        match s {
            "super" => Quality::Super,
            "lossless" => Quality::Lossless,
            "standard" => Quality::Standard,
            _ => Quality::High,
        }
    }
}

/// 曲目可用性。UI 用它决定徽标与是否可点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// 尚未探测
    #[default]
    Unknown,
    /// 可完整播放/下载
    Available,
    /// 服务端只给试听片段
    PreviewOnly,
    /// 会员专享：搜索接口的 PAY 字段直接给出，无需额外请求
    VipOnly,
    /// 已下架或取链失败
    Unavailable,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Track {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub album: String,
    pub album_id: String,
    pub duration_sec: u32,
    pub cover: Option<String>,
    #[serde(default)]
    pub availability: Availability,
    /// 便于 UI 直接展示的额外标记（如「Live」「合作」）
    #[serde(default)]
    pub subtitle: Option<String>,
}

impl Track {
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider, self.id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchPage {
    pub total: u32,
    pub page: u32,
    pub limit: u32,
    pub items: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackDetail {
    pub track: Track,
    #[serde(default)]
    pub lyrics: Vec<LyricLine>,
    /// 试听片段区间（服务端给出时才有）
    #[serde(default)]
    pub preview_range_ms: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub url: String,
    pub quality: Quality,
    pub bytes: Option<u64>,
    pub content_type: String,
    /// 探测后确认的可用性
    pub availability: Availability,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
    pub cover: Option<String>,
    pub year: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtistPage {
    pub artist_id: String,
    pub name: String,
    pub pic: Option<String>,
    /// 歌手简介（来自 PC 歌手页，可能很长）
    #[serde(default)]
    pub desc: Option<String>,
    /// 基本资料：生日 / 地区 / 身高 / 星座 之类的键值对
    #[serde(default)]
    pub meta: Vec<(String, String)>,
    pub albums: Vec<AlbumRef>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlbumPage {
    pub album_id: String,
    pub name: String,
    pub artist: String,
    pub cover: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    /// 该平台匿名档最高能拿到的音质
    pub max_anonymous_quality: Quality,
}

/// 所有平台实现的统一接口。
#[async_trait::async_trait]
pub trait MusicProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn info(&self) -> ProviderInfo;

    async fn search(&self, kw: &str, page: u32, limit: u32) -> crate::Result<SearchPage>;

    /// 取流地址。实现内部负责占位文件识别与音质降级。
    async fn resolve(&self, id: &str, q: Quality) -> crate::Result<StreamInfo>;

    /// 详情：传入已知曲目，返回补全歌词等内容的结果。
    /// 这样在详情接口被限流时可优雅降级（只用已有元数据）。
    async fn detail(&self, track: &Track) -> crate::Result<TrackDetail>;

    async fn artist(&self, artist_id: &str, name: &str, page: u32) -> crate::Result<ArtistPage>;

    async fn album(&self, album_id: &str, name: &str, page: u32) -> crate::Result<AlbumPage>;

    // ---------- 歌单 ----------

    async fn search_playlists(
        &self,
        _kw: &str,
        _page: u32,
        _limit: u32,
    ) -> crate::Result<PlaylistPage> {
        Err(crate::Error::Unsupported("该平台未实现歌单搜索".into()))
    }

    async fn playlist(&self, _id: &str) -> crate::Result<PlaylistDetail> {
        Err(crate::Error::Unsupported("该平台未实现歌单详情".into()))
    }

    /// 账号的「喜欢」歌单（各平台默认收藏）。
    ///
    /// 酷我这对应 `type=MYFAVORITE` 的那张歌单，只需 uid 即可读取。
    async fn favorites(&self, _uid: &str) -> crate::Result<PlaylistDetail> {
        Err(crate::Error::Unsupported("该平台不支持账号收藏".into()))
    }

    /// 按平台用户 id 列出其歌单。
    ///
    /// 酷我的 `pl.svc?op=getuserlist` 只需要 uid，不需要登录，
    /// 所以这是「不登录也能看自己歌单」的路径。
    async fn user_playlists(
        &self,
        _uid: &str,
        _limit: u32,
    ) -> crate::Result<Vec<PlaylistRef>> {
        Err(crate::Error::Unsupported("该平台不支持按 uid 列出歌单".into()))
    }

    // ---------- 账号 ----------
    //
    // 全部给默认实现，平台按需覆盖。这样新增平台时不必被迫实现登录。

    /// 平台能力。UI 据此决定显示哪些入口。
    fn features(&self) -> ProviderFeatures {
        ProviderFeatures::default()
    }

    /// 返回该平台支持的登录方式。空 vec 表示完全不需要登录。
    fn auth_capabilities(&self) -> Vec<AuthCapability> {
        Vec::new()
    }

    async fn login_password(&self, _user: &str, _pass: &str) -> crate::Result<Session> {
        Err(crate::Error::Unsupported("该平台不支持密码登录".into()))
    }

    /// 用已有会话 cookie 恢复登录。这是跨平台通用的逃生通道：
    /// 参数是浏览器 DevTools 里复制出来的 `k1=v1; k2=v2` 形式。
    async fn login_cookie(&self, _cookie: &str) -> crate::Result<Session> {
        Err(crate::Error::Unsupported("该平台不支持 cookie 登录".into()))
    }

    /// 当前登录用户自己的歌单。未实现或需要签名接口时返回 Unsupported。
    async fn my_playlists(&self, _session: &Session) -> crate::Result<Vec<PlaylistRef>> {
        Err(crate::Error::Unsupported("该平台不提供用户歌单接口".into()))
    }

    async fn logout(&self, _session: &Session) -> crate::Result<()> {
        Ok(())
    }
}

// ============ 歌单 ============

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaylistRef {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub cover: Option<String>,
    pub track_count: u32,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub play_count: u64,
    pub tags: Option<String>,
}

impl PlaylistRef {
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider, self.id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaylistPage {
    pub total: u32,
    pub page: u32,
    pub items: Vec<PlaylistRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaylistDetail {
    pub playlist: PlaylistRef,
    pub tracks: Vec<Track>,
}

// ============ 账号 ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Password,
    Cookie,
    QrCode,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::Cookie => "cookie",
            AuthMethod::QrCode => "qr_code",
        }
    }
}

/// 某平台支持的登录方式及其可用性。
///
/// `supported=false` 时 `reason` 必须说明**为什么**，UI 直接展示给用户，
/// 而不是给一个点了必错的假登录框。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCapability {
    pub method: AuthMethod,
    pub supported: bool,
    pub reason: Option<String>,
}

impl AuthCapability {
    pub fn ok(method: AuthMethod) -> Self {
        Self { method, supported: true, reason: None }
    }
    pub fn blocked(method: AuthMethod, reason: impl Into<String>) -> Self {
        Self { method, supported: false, reason: Some(reason.into()) }
    }
}

/// 平台能力声明。
///
/// UI 依据它决定显示哪些功能 —— 例如平台没有「喜欢」集合时，
/// 就不显示喜欢入口，而不是给一个点了报错的页面。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderFeatures {
    /// 平台是否提供账号级的「喜欢」集合
    pub favorites: bool,
}

/// 通用会话。跨平台一致，平台私有字段放 `extra`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub provider: String,
    pub user_id: String,
    pub nickname: String,
    pub avatar: Option<String>,
    /// 任意平台的 cookie（键值对），便于将来直接复用到新 provider
    pub cookies: Vec<(String, String)>,
    /// 平台私有凭据：loginSid / token / vip 等级等
    pub extra: std::collections::HashMap<String, String>,
    pub created_at: i64,
}

impl Session {
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ")
    }
}
