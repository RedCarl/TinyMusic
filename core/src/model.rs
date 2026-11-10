use serde::{Deserialize, Serialize};

/// 音质档。所有平台/音源共用同一套档位语义。
///
/// 酷我当前可直接取到的是 `standard` / `super` / `lossless`；
/// `hires` / `atmos` / `master` 已在模型和 UI 中预留，但匿名公开链路
/// 拿不到（需要会员态 + 加密音频解密），因此默认不可选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Quality {
    /// 标准 128k 有损
    #[default]
    #[serde(rename = "standard")]
    Standard,
    /// 高品质 320k 有损
    #[serde(rename = "super")]
    Super,
    /// 无损 FLAC/ALAC
    #[serde(rename = "lossless")]
    Lossless,
    /// 高解析度（24bit / 高采样率）
    #[serde(rename = "hires")]
    HiRes,
    /// 杜比全景声
    #[serde(rename = "atmos")]
    Atmos,
    /// 母带
    #[serde(rename = "master")]
    Master,
}

impl Quality {
    /// 目标码率（无损/高级档用 `None`，避免把“估算码率”当成硬指标）。
    pub fn nominal_kbps(self) -> Option<u32> {
        match self {
            Quality::Standard => Some(128),
            Quality::Super => Some(320),
            Quality::Lossless => None,
            Quality::HiRes | Quality::Atmos | Quality::Master => None,
        }
    }

    /// 用于“请求档位 → 实际档位”的降级比较；高级档位不参与自动降级。
    pub fn rank(self) -> u8 {
        match self {
            Quality::Standard => 0,
            Quality::Super => 1,
            Quality::Lossless => 2,
            Quality::HiRes => 3,
            Quality::Atmos => 4,
            Quality::Master => 5,
        }
    }

    pub fn is_advanced(self) -> bool {
        matches!(self, Quality::HiRes | Quality::Atmos | Quality::Master)
    }

    pub fn is_lossless(self) -> bool {
        self == Quality::Lossless
    }

    /// 降级顺序：无损 → 320 → 128 → 无。高级档位不自动降级。
    pub fn next_lower(self) -> Option<Quality> {
        match self {
            Quality::Lossless => Some(Quality::Super),
            Quality::Super => Some(Quality::Standard),
            Quality::Standard => None,
            Quality::HiRes | Quality::Atmos | Quality::Master => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Standard => "standard",
            Quality::Super => "super",
            Quality::Lossless => "lossless",
            Quality::HiRes => "hires",
            Quality::Atmos => "atmos",
            Quality::Master => "master",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Quality::Standard => "标准 128k",
            Quality::Super => "高品质 320k",
            Quality::Lossless => "无损 FLAC",
            Quality::HiRes => "Hi-Res",
            Quality::Atmos => "杜比全景声",
            Quality::Master => "母带",
        }
    }

    /// 解析 wire 值。`high` 是旧版本遗留值，按旧语义映射到 128k。
    pub fn parse(s: &str) -> Option<Quality> {
        match s.trim().to_ascii_lowercase().as_str() {
            "standard" | "high" => Some(Quality::Standard),
            "super" => Some(Quality::Super),
            "lossless" => Some(Quality::Lossless),
            "hires" | "hi_res" | "hi-res" => Some(Quality::HiRes),
            "atmos" | "dolby" | "dolby_atmos" => Some(Quality::Atmos),
            "master" => Some(Quality::Master),
            _ => None,
        }
    }

    /// 旧代码里的默认值语义：未知值按 128k 处理。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Quality {
        Quality::parse(s).unwrap_or(Quality::Standard)
    }
}

/// 音频编码格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Mp3,
    Aac,
    Flac,
    Alac,
    Ogg,
    Wav,
    /// 其他/未知
    #[default]
    Unknown,
}

impl Codec {
    pub fn extension(self) -> &'static str {
        match self {
            Codec::Mp3 => "mp3",
            Codec::Aac | Codec::Alac => "m4a",
            Codec::Flac => "flac",
            Codec::Ogg => "ogg",
            Codec::Wav => "wav",
            Codec::Unknown => "mp3",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(self, Codec::Flac | Codec::Alac | Codec::Wav)
    }
}

/// 实际音频格式。由 `quality::verify_url` 通过文件头/Content-Type/字节数推断。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioFormat {
    pub codec: Codec,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bits_per_sample: Option<u8>,
    pub channels: Option<u8>,
    pub lossless: bool,
    /// 是否由文件头/时长校验确认（false = 只知道 Content-Type）
    #[serde(default)]
    pub verified: bool,
}

impl AudioFormat {
    pub fn extension(&self) -> &'static str {
        self.codec.extension()
    }

    /// 按实际码率归档位；未知时返回 None（调用方保留请求档位并标未校验）。
    pub fn tier(&self) -> Option<Quality> {
        if self.lossless {
            return Some(Quality::Lossless);
        }
        match self.bitrate_kbps {
            Some(k) if k >= 256 => Some(Quality::Super),
            Some(_) => Some(Quality::Standard),
            None => None,
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
    /// 搜索热度（酷我搜索返回的 SCORE100，0 表示未知）
    #[serde(default)]
    pub hot: u32,
    /// 从列表接口的 MINFO 解析出的、当前真正可播放的档位。
    /// 空表示未知（需要 `track_qualities` 再探测）。
    #[serde(default)]
    pub available_qualities: Vec<Quality>,
    /// 目录里存在加密高级格式（Hi-Res / 全景声 / 母带），但当前版本不可播。
    #[serde(default)]
    pub has_advanced: bool,
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
    /// 实际档位（经过校验/降级后的结果）
    pub quality: Quality,
    /// 用户请求的档位
    #[serde(default)]
    pub requested: Quality,
    pub bytes: Option<u64>,
    pub content_type: String,
    /// 探测后确认的可用性
    pub availability: Availability,
    /// 实际编码/采样率/位深
    #[serde(default)]
    pub format: AudioFormat,
    /// 实际走的链路：anti / trackmedia / ...
    #[serde(default)]
    pub via: String,
    /// 是否因目标档位不可用而降级
    #[serde(default)]
    pub degraded: bool,
    /// 是否完成文件头/时长校验
    #[serde(default)]
    pub verified: bool,
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
    /// 该平台当前可播放的原生档位
    pub qualities: Vec<Quality>,
}

/// 某一首曲目的音质探测结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackQualities {
    /// 当前真正可播放的档位
    pub available: Vec<Quality>,
    /// 目录里有加密高级格式（当前不可播）
    pub has_advanced: bool,
    /// 是否成功探测（false 表示超时/失败，UI 不应据此禁用）
    pub known: bool,
}

/// 给设置页/菜单用的档位说明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityCapability {
    pub value: Quality,
    pub label: String,
    pub kbps: Option<u32>,
    pub lossless: bool,
    pub available: bool,
    pub reason: Option<String>,
}

/// 所有平台实现的统一接口。
#[async_trait::async_trait]
pub trait MusicProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn info(&self) -> ProviderInfo;

    async fn search(&self, kw: &str, page: u32, limit: u32) -> crate::Result<SearchPage>;

    /// 取流地址。实现内部负责占位文件识别与音质降级。
    /// 传入整首 `Track`，这样时长校验/曲目能力都能用上。
    async fn resolve(&self, track: &Track, q: Quality) -> crate::Result<StreamInfo>;

    /// 探测某一首曲目真正可播放的档位（列表接口没带 MINFO 时使用）。
    async fn track_qualities(&self, _track: &Track) -> crate::Result<TrackQualities> {
        Err(crate::Error::Unsupported("该平台不支持音质探测".into()))
    }

    /// 清掉某首曲目的取链缓存（播放失败/强制刷新时用）。
    fn invalidate_resolve(&self, _id: &str) {}

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

    /// 「每日推荐歌曲」之类的个性化推荐。需要登录态，未登录应返回可读错误。
    async fn recommend_songs(&self, _limit: u32) -> crate::Result<Vec<Track>> {
        Err(crate::Error::Unsupported("该平台不提供每日推荐".into()))
    }

    /// 「推荐歌单」。多数平台匿名也能拿到，有登录态时更个性化。
    async fn recommend_playlists(&self) -> crate::Result<Vec<PlaylistRef>> {
        Err(crate::Error::Unsupported("该平台不提供推荐歌单".into()))
    }

    /// 按平台用户 id 列出其歌单。
    ///
    /// 酷我的 `pl.svc?op=getuserlist` 只需要 uid，不需要登录，
    /// 所以这是「不登录也能看自己歌单」的路径。
    async fn user_playlists(&self, _uid: &str, _limit: u32) -> crate::Result<Vec<PlaylistRef>> {
        Err(crate::Error::Unsupported(
            "该平台不支持按 uid 列出歌单".into(),
        ))
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

    /// 把已持久化的会话注入 provider。
    ///
    /// 需要 cookie 才有意义的平台（网易云）在启动时由外壳调用一次，
    /// 否则「重启后仍需重新登录」。默认实现忽略。
    fn set_session(&self, _session: &Session) {}
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
        Self {
            method,
            supported: true,
            reason: None,
        }
    }
    pub fn blocked(method: AuthMethod, reason: impl Into<String>) -> Self {
        Self {
            method,
            supported: false,
            reason: Some(reason.into()),
        }
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
    /// 平台是否提供「每日推荐 / 推荐歌单」入口
    #[serde(default)]
    pub recommend: bool,
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
