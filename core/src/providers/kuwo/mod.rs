//! 酷我 provider —— 基于逆向出的匿名接口实现。
//!
//! 设计约束（全部来自实测）：
//!
//! 1. 搜索接口返回 Python dict 字面量，需用 `kwjson` 解析。
//! 2. `anti.s`（convert_url3）是**账号策略**接口：匿名请求会员曲的 320k/无损时，
//!    服务端返回固定占位文件（URL 含 `588957081`、181,521 字节、约 11.3 秒）。
//! 3. 因此「能取到直链」不等于「能听到歌」，`anti.s` 的结果必须探测校验。
//! 4. 占位命中后改走**资源管理链路**（`resolve_trackmedia`）：
//!    musicinfo 拿 S1/S2 → 加密请求 `yl_res_manage.search` → 解出 trackmedia 直链。
//!    这条链路服务端只校验请求合法性，与 VIP 状态无关，会员曲也能拿到完整音频。
//! 5. `payInfo` / 搜索结果的 `PAY` 字段不可作为「不可播放」判据
//!    （`PAY=16711935` 只代表会员内容，实测仍能通过 4 取到整曲）。
//! 6. 艺人页 / 专辑页不依赖额外端点，用搜索结果按 `ARTISTID` / `ALBUMID` 归组。

pub mod crypt;

use crate::cache::TtlCache;
use crate::error::{Error, Result};
use crate::http::{HttpClient, ProbeInfo};
use crate::kwjson;
use crate::model::*;
use crate::quality;
use crate::text::clean;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

pub const SEARCH_HOST: &str = "search.kuwo.cn";
pub const RESOLVE_HOST: &str = "antiserver.kuwo.cn";
pub const DETAIL_HOST: &str = "m.kuwo.cn";

pub const PLAYLIST_HOST: &str = "nplserver.kuwo.cn";
/// 资源管理服务：签发 trackmedia 直链
pub const RES_MANAGE_HOST: &str = "deliver.kuwo.cn";

pub const COVER_BASE: &str = "https://img1.kuwo.cn/star/albumcover/";

/// 歌手资料：头像 + 简介 + 基本资料
#[derive(Debug, Clone, Default)]
pub struct SingerProfile {
    pub name: Option<String>,
    pub pic: Option<String>,
    pub bio: Option<String>,
    pub meta: Vec<(String, String)>,
}

/// 从 PC 歌手页的 `window.__NUXT__` 里抠出歌手资料。
///
/// 页面里的 `singerInfo:{birthday:"...",info:"..."}` 是 JS 对象字面量，
/// 键不带引号、字符串里还有 `\u002F` / `\n` 这类转义，不是合法 JSON，
/// 所以这里按字段名扫描并逐字符反转义。
pub fn parse_singer_page(html: &str) -> SingerProfile {
    let Some(start) = html.find("singerInfo:{") else {
        return SingerProfile::default();
    };
    let rest = &html[start..];
    // 对象是内联的，遇到 `}}` 就是结尾；给个上限防御异常页面
    let end = rest
        .find("}}")
        .map(|e| e + 2)
        .unwrap_or(rest.len().min(120_000));
    let obj = &rest[..end];

    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| js_field(obj, k))
            .filter(|s| !s.is_empty())
    };

    let pic = pick(&["pic300", "pic120", "pic70"])
        .map(|s| crate::text::clean(&s))
        .filter(|s| s.starts_with("http"));
    let bio = pick(&["info"])
        .map(|s| crate::text::clean(&s))
        .filter(|s| !s.is_empty());

    let mut meta = Vec::new();
    for (label, key) in [
        ("外文名", "aartist"),
        ("生日", "birthday"),
        ("地区", "country"),
        ("出生地", "birthplace"),
        ("身高", "tall"),
        ("体重", "weight"),
        ("星座", "constellation"),
        ("语言", "language"),
    ] {
        if let Some(v) = pick(&[key]) {
            let v = crate::text::clean(&v);
            if !v.is_empty() && v != "0" && v != "0cm" && v != "0kg" {
                meta.push((label.to_string(), v));
            }
        }
    }

    let name = pick(&["name"])
        .map(|s| crate::text::clean(&s))
        .filter(|s| !s.is_empty());

    SingerProfile {
        name,
        pic,
        bio,
        meta,
    }
}

/// 在 JS 对象字面量里取 `key:"value"` 的字符串值（处理 JS 转义）
fn js_field(obj: &str, key: &str) -> Option<String> {
    let pat = format!("{}:\"", key);
    let mut search = 0;
    while let Some(pos) = obj[search..].find(&pat) {
        let abs = search + pos;
        // 必须是完整字段名：前面是 { 或 , 或空格
        let boundary_ok = abs == 0 || matches!(obj.as_bytes()[abs - 1], b'{' | b',' | b' ');
        if !boundary_ok {
            search = abs + pat.len();
            continue;
        }

        let chars: Vec<char> = obj[abs + pat.len()..].chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '"' {
                return Some(out);
            }
            if c == '\\' {
                let Some(n) = chars.get(i + 1).copied() else {
                    break;
                };
                match n {
                    'u' if i + 5 < chars.len() => {
                        let hex: String = chars[i + 2..i + 6].iter().collect();
                        match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            Some(ch) => out.push(ch),
                            None => out.push(' '),
                        }
                        i += 6;
                        continue;
                    }
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push(' '),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    other => out.push(other),
                }
                i += 2;
                continue;
            }
            out.push(c);
            i += 1;
        }
        return None;
    }
    None
}
pub const ARTIST_PIC_BASE: &str = "https://img1.kuwo.cn/star/starheads/";

/// 服务端对「无权益」请求固定下发的占位资源
pub const PLACEHOLDER_PATH_ID: &str = "588957081";
pub const PLACEHOLDER_BYTES: u64 = 181_521;

pub struct KuwoProvider {
    http: Arc<HttpClient>,
    search_cache: TtlCache<String, SearchPage>,
    detail_cache: TtlCache<String, TrackDetail>,
    resolve_cache: TtlCache<String, StreamInfo>,
    quality_cache: TtlCache<String, TrackQualities>,
    profile_cache: TtlCache<String, SingerProfile>,
}

impl Default for KuwoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl KuwoProvider {
    pub fn new() -> Self {
        Self::with_http(Arc::new(HttpClient::new(700, 3)))
    }

    pub fn with_http(http: Arc<HttpClient>) -> Self {
        Self {
            http,
            search_cache: TtlCache::new(Duration::from_secs(600), 256),
            detail_cache: TtlCache::new(Duration::from_secs(6 * 3600), 256),
            // CDN 直链会提前失效，取链缓存不能放 24 小时
            resolve_cache: TtlCache::new(Duration::from_secs(20 * 60), 512),
            quality_cache: TtlCache::new(Duration::from_secs(10 * 60), 256),
            profile_cache: TtlCache::new(Duration::from_secs(24 * 3600), 128),
        }
    }

    // ---------- URL 构造 ----------

    fn search_url(kw: &str, page: u32, limit: u32) -> String {
        format!(
            "http://{}/r.s?client=kt&all={}&pn={}&rn={}&ft=music&rformat=json&encoding=utf8&vipver=1",
            SEARCH_HOST,
            urlencode(kw),
            page,
            limit.clamp(1, 100)
        )
    }

    fn resolve_url(id: &str, br: &str) -> String {
        format!(
            "http://{}/anti.s?type=convert_url3&rid=MUSIC_{}&format=mp3&response=url&br={}",
            RESOLVE_HOST, id, br
        )
    }

    fn search_playlist_url(kw: &str, page: u32, limit: u32) -> String {
        format!(
            "http://{}/r.s?client=kt&all={}&pn={}&rn={}&ft=playlist&rformat=json&encoding=utf8",
            SEARCH_HOST,
            urlencode(kw),
            page,
            limit.clamp(1, 100)
        )
    }

    fn user_playlist_url(uid: &str, pn: u32, rn: u32) -> String {
        format!(
            "http://{}/pl.svc?op=getuserlist&uid={}&encode=utf8&pn={}&rn={}&keyset=pl2012&identity=kuwo&pcmp4=1&vipver=1",
            PLAYLIST_HOST, uid, pn, rn.clamp(1, 100)
        )
    }

    /// 解析 `op=getuserlist` 的响应。
    ///
    /// 这条接口**只需要 uid**，不需要登录也不需要 web 端签名，
    /// 连 `ispub=false` 的私密歌单也会返回 —— 这是「不登录看自己歌单」的可行路径。
    pub fn parse_user_playlists(body: &str) -> Result<Vec<PlaylistRef>> {
        let v: Value = serde_json::from_str(body)
            .map_err(|e| Error::Parse(format!("用户歌单响应不是 JSON: {e}")))?;
        let items = v
            .get("plist")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|it| {
                        let title = kwjson::s(it, "title");
                        let pic = kwjson::s(it, "pic");
                        let tag = kwjson::s(it, "tag");
                        PlaylistRef {
                            provider: "kuwo".into(),
                            id: kwjson::s(it, "id"),
                            name: if title.is_empty() {
                                "未命名歌单".to_string()
                            } else {
                                clean(&title)
                            },
                            cover: if pic.is_empty() {
                                None
                            } else {
                                Some(normalize_cdn(&pic))
                            },
                            track_count: kwjson::u(it, "musicnum"),
                            creator: None,
                            description: None,
                            play_count: 0,
                            tags: if tag.is_empty() { None } else { Some(tag) },
                        }
                    })
                    .filter(|p| !p.id.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(items)
    }

    fn playlist_url(pid: &str, pn: u32, rn: u32) -> String {
        format!(
            "http://{}/pl.svc?op=getlistinfo&pid={}&pn={}&rn={}&encode=utf8&keyset=pl2012&identity=kuwo&pcmp4=1&vipver=1",
            PLAYLIST_HOST, pid, pn, rn
        )
    }

    fn detail_url(id: &str) -> String {
        format!(
            "http://{}/newh5/singles/songinfoandlrc?musicId={}",
            DETAIL_HOST, id
        )
    }

    // ---------- 字段映射 ----------

    /// 歌单搜索结果解析
    pub fn parse_playlist_search(body: &str, page: u32) -> Result<PlaylistPage> {
        let v = kwjson::parse(body)?;
        let total = kwjson::u(&v, "TOTAL");
        let items = v
            .get("abslist")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|it| kwjson::s(it, "DC_TARGETTYPE") == "songlist")
                    .map(Self::map_playlist)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(PlaylistPage { total, page, items })
    }

    pub fn map_playlist(v: &Value) -> PlaylistRef {
        let id = first_nonempty(&[kwjson::s(v, "playlistid"), kwjson::s(v, "DC_TARGETID")]);
        let cover = first_nonempty(&[kwjson::s(v, "hts_pic"), kwjson::s(v, "pic")]);
        let creator = kwjson::s(v, "nickname");
        let desc = clean(&kwjson::s(v, "intro"));
        let tags = kwjson::s(v, "tags");
        PlaylistRef {
            provider: "kuwo".into(),
            id,
            name: clean(&kwjson::s(v, "name")),
            cover: if cover.is_empty() {
                None
            } else {
                Some(normalize_cdn(&cover))
            },
            track_count: kwjson::u(v, "songnum"),
            creator: if creator.is_empty() {
                None
            } else {
                Some(clean(&creator))
            },
            description: if desc.is_empty() { None } else { Some(desc) },
            play_count: kwjson::s(v, "playcnt").parse::<u64>().unwrap_or(0),
            tags: if tags.is_empty() { None } else { Some(tags) },
        }
    }

    /// 从列表项的 `MINFO` / `N_MINFO` / `FORMATS` 提取音质能力。
    fn quality_fields(v: &Value) -> (Vec<Quality>, bool) {
        let minfo = kwjson::s(v, "MINFO");
        let nminfo = kwjson::s(v, "N_MINFO");
        let formats = kwjson::s(v, "FORMATS");
        let mut available = quality::qualities_from_minfo(&minfo);
        if available.is_empty() {
            available = quality::qualities_from_formats(&formats);
        }
        let has_advanced =
            quality::has_advanced_formats(&nminfo) || quality::has_advanced_formats(&formats);
        (available, has_advanced)
    }

    /// 歌单详情接口返回的曲目字段是小写命名，与搜索结果不同，单独映射
    pub fn map_playlist_track(v: &Value) -> Track {
        let pic = kwjson::s(v, "albumpic");
        let (available_qualities, has_advanced) = Self::quality_fields(v);
        Track {
            provider: "kuwo".into(),
            id: kwjson::s(v, "id"),
            title: clean(&kwjson::s(v, "name")),
            artist: clean(&kwjson::s(v, "artist")),
            artist_id: kwjson::s(v, "artistid"),
            album: clean(&kwjson::s(v, "album")),
            album_id: kwjson::s(v, "albumid"),
            duration_sec: kwjson::u(v, "duration"),
            cover: if pic.is_empty() {
                None
            } else {
                Some(normalize_cdn(&pic))
            },
            availability: if v.get("online").and_then(|x| x.as_u64()).unwrap_or(1) == 0 {
                Availability::Unavailable
            } else if v.get("pay").and_then(|x| x.as_u64()).unwrap_or(0) as u32 == PAY_VIP_ONLY {
                Availability::VipOnly
            } else {
                Availability::Unknown
            },
            subtitle: None,
            hot: 0,
            available_qualities,
            has_advanced,
        }
    }

    /// 把详情响应整体转成 PlaylistDetail
    pub fn parse_playlist_detail(body: &str, pid: &str) -> Result<PlaylistDetail> {
        let v: Value = serde_json::from_str(body)
            .map_err(|e| Error::Parse(format!("歌单响应不是 JSON: {e}")))?;
        if v.get("result").and_then(|r| r.as_str()) == Some("fail") {
            return Err(Error::NotFound(format!("歌单 {pid} 不存在或不可见")));
        }
        let pic = kwjson::s(&v, "pic");
        let info = clean(&kwjson::s(&v, "info"));
        let uname = kwjson::s(&v, "uname");
        let tag = kwjson::s(&v, "tag");
        let playlist = PlaylistRef {
            provider: "kuwo".into(),
            id: pid.to_string(),
            name: clean(&kwjson::s(&v, "title")),
            cover: if pic.is_empty() {
                None
            } else {
                Some(normalize_cdn(&pic))
            },
            track_count: kwjson::u(&v, "total"),
            creator: if uname.is_empty() {
                None
            } else {
                Some(clean(&uname))
            },
            description: if info.is_empty() { None } else { Some(info) },
            play_count: kwjson::s(&v, "playnum").parse::<u64>().unwrap_or(0),
            tags: if tag.is_empty() { None } else { Some(tag) },
        };
        let tracks = v
            .get("musiclist")
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().map(Self::map_playlist_track).collect())
            .unwrap_or_default();
        Ok(PlaylistDetail { playlist, tracks })
    }

    /// 把搜索结果的单项转成统一 Track
    pub fn map_item(v: &Value) -> Track {
        let title = first_nonempty(&[kwjson::s(v, "SONGNAME"), kwjson::s(v, "NAME")]);
        let cover_key = kwjson::s(v, "web_albumpic_short");
        let subtitle = {
            let st = kwjson::s(v, "SUBTITLE");
            if st.is_empty() {
                None
            } else {
                Some(clean(&st))
            }
        };
        let (available_qualities, has_advanced) = Self::quality_fields(v);
        Track {
            provider: "kuwo".into(),
            id: kwjson::s(v, "DC_TARGETID"),
            title: clean(&title),
            artist: clean(&kwjson::s(v, "ARTIST")),
            artist_id: first_nonempty(&[kwjson::s(v, "ARTISTID"), kwjson::s(v, "allartistid")]),
            album: clean(&kwjson::s(v, "ALBUM")),
            album_id: kwjson::s(v, "ALBUMID"),
            duration_sec: kwjson::u(v, "DURATION"),
            cover: if cover_key.is_empty() {
                None
            } else {
                Some(format!("{}{}", COVER_BASE, cover_key))
            },
            availability: probe_availability_hint(v),
            subtitle,
            hot: kwjson::u(v, "SCORE100"),
            available_qualities,
            has_advanced,
        }
    }

    /// 解析搜索响应的原始文本 → SearchPage
    pub fn parse_search(body: &str, page: u32, limit: u32) -> Result<SearchPage> {
        let v = kwjson::parse(body)?;
        let total = kwjson::u(&v, "TOTAL");
        let mut items = v
            .get("abslist")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().map(Self::map_item).collect::<Vec<_>>())
            .unwrap_or_default();
        // 按热度（站点搜索分）从高到低，分页内排序
        items.sort_by(|a, b| b.hot.cmp(&a.hot));
        Ok(SearchPage {
            total,
            page,
            limit,
            items,
        })
    }

    /// 解析取链响应 → 直链
    pub fn parse_resolve(body: &str) -> Result<String> {
        let v: Value = serde_json::from_str(body)
            .map_err(|e| Error::Parse(format!("取链响应不是 JSON: {e}")))?;
        let url = v.get("url").and_then(|u| u.as_str()).unwrap_or("");
        if url.is_empty() || url == "failed" {
            return Err(Error::NotFound("取链返回空或 failed".into()));
        }
        Ok(url.to_string())
    }

    /// 解析详情响应 → TrackDetail（歌词 + 试听区间）
    pub fn parse_detail(body: &str, base: Track) -> Result<TrackDetail> {
        let v: Value = serde_json::from_str(body)
            .map_err(|e| Error::Parse(format!("详情响应不是 JSON: {e}")))?;
        let data = v
            .get("data")
            .ok_or_else(|| Error::Parse("详情响应缺少 data".into()))?;
        let si = data.get("songinfo").unwrap_or(&Value::Null);

        let mut track = base;
        let name = kwjson::s(si, "name");
        if !name.is_empty() {
            track.title = clean(&name);
        }
        let artist = kwjson::s(si, "artist");
        if !artist.is_empty() {
            track.artist = clean(&artist);
        }
        let album = kwjson::s(si, "album");
        if !album.is_empty() {
            track.album = clean(&album);
        }
        let dur = kwjson::u(si, "duration");
        if dur > 0 {
            track.duration_sec = dur;
        }
        let pic = kwjson::s(si, "pic");
        if !pic.is_empty() {
            track.cover = Some(pic);
        }

        let lyrics = data
            .get("lrclist")
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|line| {
                        let text = clean(&kwjson::s(line, "lineLyric"));
                        if text.is_empty() {
                            return None;
                        }
                        let t = kwjson::s(line, "time");
                        let ms = (t.parse::<f64>().unwrap_or(0.0) * 1000.0).max(0.0) as u32;
                        Some(LyricLine { time_ms: ms, text })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(TrackDetail {
            track,
            lyrics,
            preview_range_ms: None,
        })
    }

    /// 占位文件判定。三级，任一命中即视为不可用。
    pub fn is_placeholder(url: &str, probe: &ProbeInfo, meta_dur: u32, kbps: u32) -> bool {
        if url.contains(PLACEHOLDER_PATH_ID) || probe.bytes == Some(PLACEHOLDER_BYTES) {
            return true;
        }
        if let Some(n) = probe.bytes {
            if meta_dur >= 30 && kbps > 0 {
                let est_sec = (n as f64 * 8.0) / (kbps as f64 * 1000.0);
                if est_sec * 100.0 < (meta_dur as f64) * 60.0 {
                    return true;
                }
            }
        }
        false
    }

    /// 取链并按需降级：先 anti，再 trackmedia；实际档位以校验结果为准。
    async fn resolve_with_fallback(&self, track: &Track, want: Quality) -> Result<StreamInfo> {
        if want.is_advanced() {
            return Err(Error::Unsupported(format!(
                "{} 需要会员账号与加密音频解密，当前未接入",
                want.label()
            )));
        }

        let mut best: Option<StreamInfo> = None;
        let mut tried = Vec::new();
        let mut q = Some(want);

        while let Some(cur) = q {
            if let Some(b) = &best {
                if b.quality.rank() >= cur.rank() {
                    break;
                }
            }

            let key = format!("{}:{}", track.id, cur.as_str());
            if let Some(hit) = self.resolve_cache.get(&key) {
                if hit.availability == Availability::Available {
                    if hit.quality.rank() >= want.rank() {
                        return Ok(hit);
                    }
                    keep_best(&mut best, hit);
                }
                tried.push(cur);
                q = cur.next_lower();
                continue;
            }

            let mut got: Option<StreamInfo> = None;

            // 1) anti.s 轻量直连（128/320/FLAC；无损通常返回占位）
            if let Some(br) = Self::br_for(cur) {
                if let Ok(body) = self
                    .http
                    .get_text(RESOLVE_HOST, &Self::resolve_url(&track.id, br))
                    .await
                {
                    if let Ok(direct) = Self::parse_resolve(&body) {
                        if let Ok(info) = quality::verify_url(
                            &self.http,
                            &direct,
                            track.duration_sec,
                            cur,
                            "anti",
                        )
                        .await
                        {
                            got = Some(info);
                        }
                    }
                }
            }

            // 2) 资源管理链路（anti 占位/无直链时；也是无损的唯一来源）
            if got
                .as_ref()
                .map(|i| i.quality.rank() < cur.rank())
                .unwrap_or(true)
            {
                if let Ok(Some(info)) = self.resolve_trackmedia(track, cur).await {
                    if got
                        .as_ref()
                        .map(|g| info.quality.rank() > g.quality.rank())
                        .unwrap_or(true)
                    {
                        got = Some(info);
                    }
                }
            }

            if let Some(mut info) = got {
                info.requested = want;
                info.degraded = info.quality.rank() < want.rank();
                if info.quality.rank() >= want.rank() {
                    self.resolve_cache.put(key, info.clone());
                    return Ok(info);
                }
                keep_best(&mut best, info);
            } else {
                tried.push(cur);
            }

            q = cur.next_lower();
        }

        if let Some(mut info) = best {
            info.requested = want;
            info.degraded = info.quality.rank() < want.rank();
            self.resolve_cache
                .put(format!("{}:{}", track.id, want.as_str()), info.clone());
            return Ok(info);
        }

        let _ = tried;
        Err(Error::Unavailable)
    }
}

fn keep_best(best: &mut Option<StreamInfo>, info: StreamInfo) {
    if best
        .as_ref()
        .map(|b| info.quality.rank() > b.quality.rank())
        .unwrap_or(true)
    {
        *best = Some(info);
    }
}

impl KuwoProvider {
    /// 音质 → musicinfo 里的格式名。高级档位当前没有可用的明文格式。
    fn fmt_for(q: Quality) -> Option<&'static str> {
        match q {
            Quality::Standard => Some("MP3128"),
            Quality::Super => Some("MP3H"),
            Quality::Lossless => Some("ALFLAC"),
            Quality::HiRes | Quality::Atmos | Quality::Master => None,
        }
    }

    /// 音质 → anti.s 的 br 参数。
    fn br_for(q: Quality) -> Option<&'static str> {
        match q {
            Quality::Standard => Some("128kmp3"),
            Quality::Super => Some("320kmp3"),
            Quality::Lossless => Some("2000kflac"),
            Quality::HiRes | Quality::Atmos | Quality::Master => None,
        }
    }

    async fn musicinfo_text(&self, id: &str) -> Result<String> {
        let raw = self
            .http
            .get_bytes(SEARCH_HOST, &Self::musicinfo_url(id))
            .await?;
        Self::parse_musicinfo_packet(&raw)
    }

    fn musicinfo_url(id: &str) -> String {
        format!(
            "http://{}/r.s?stype=musicinfo&itemset=music_2014&alflac=1&pcmp4=1&ids=MUSIC_{}",
            SEARCH_HOST, id
        )
    }

    /// 解 musicinfo 的自定义包：`[u32 明文长][u32 压缩长][zlib 流]`
    pub fn parse_musicinfo_packet(raw: &[u8]) -> Result<String> {
        use std::io::Read;
        if raw.len() < 9 {
            return Err(Error::Parse("musicinfo 包太短".into()));
        }
        let mut z = flate2::read::ZlibDecoder::new(&raw[8..]);
        let mut out = String::new();
        z.read_to_string(&mut out)
            .map_err(|e| Error::Parse(format!("musicinfo zlib 解压失败: {e}")))?;
        Ok(out)
    }

    /// 从 musicinfo 文本里取某格式的 (S1, S2, SIZE)
    pub fn parse_sig(text: &str, fmt: &str) -> Option<(u32, u32, u64)> {
        for line in text.lines() {
            let Some(rest) = line.strip_prefix(fmt) else {
                continue;
            };
            let Some(rest) = rest.strip_prefix("=S1:") else {
                continue;
            };
            let mut it = rest.split('|');
            let s1 = it.next()?.trim().parse::<u32>().ok()?;
            let s2 = it.next()?.trim().strip_prefix("S2:")?.parse::<u32>().ok()?;
            let size = it
                .next()?
                .trim()
                .strip_prefix("SIZE:")?
                .parse::<u64>()
                .ok()?;
            return Some((s1, s2, size));
        }
        None
    }

    /// 构造 yl_res_manage 的请求明文（末尾的 CRLF 是协议的一部分）
    fn res_manage_plain(s1: u32, s2: u32) -> Vec<u8> {
        // 注意：模板里不能有任何多余空白，服务端会校验整串格式。
        // 这里刻意写成单行，避免续行缩进混进字符串。
        let body = format!(
            "<001><U_QRY>|<{s1},{s2}>|<58365624><MUSIC_9.6.0.0_W1><kwmusic_web_1_bds_20171222.exe>|<>|<rid>|<uip:0.0.0.0>|<new>|<nat:3>|<flags:262144><speer>|<ipdeny:no>|<loginid:645793966>\r\n"
        );
        body.into_bytes()
    }

    /// 从响应明文里抽 URL 与 `<FILE_LEN>`。
    ///
    /// 真实形状（注意 URL 本身被尖括号包着）：
    /// `<URL><1>|<http://kw-er.../x.flac?source=pc_player.flac>|<FILE_LEN><26012257>|...`
    pub fn parse_res_manage(text: &str) -> (Vec<String>, Option<u64>) {
        let mut urls = Vec::new();
        let mut idx = 0usize;
        while let Some(pos) = text[idx..].find("http") {
            let abs = idx + pos;
            let rest = &text[abs..];
            idx = abs + 4;
            if !(rest.starts_with("http://") || rest.starts_with("https://")) {
                continue;
            }
            let end = rest
                .find(|c: char| {
                    c.is_whitespace() || c == '>' || c == '<' || c == '|' || c.is_control()
                })
                .unwrap_or(rest.len());
            let u = &rest[..end];
            if u.len() > 12 {
                urls.push(u.to_string());
            }
        }

        let mut len = None;
        if let Some(i) = text.find("<FILE_LEN>") {
            let rest = &text[i + 10..];
            let end = rest.find('>').unwrap_or(rest.len());
            // 可能形如 `<26012257>`，两头的尖括号都要剪掉
            let raw = rest[..end].trim().trim_matches(|c| c == '<' || c == '>');
            len = raw.parse::<u64>().ok();
        }
        (urls, len)
    }

    /// 走资源管理接口取 trackmedia 直链。
    ///
    /// 这条链路不依赖账号：服务端只校验请求是否合法、
    /// CDN 只校验 URL 里的 token/时间戳，与用户 VIP 状态无关。
    /// 结果必须再经过 `quality::verify_url`，不能直接相信请求档位。
    pub async fn resolve_trackmedia(
        &self,
        track: &Track,
        q: Quality,
    ) -> Result<Option<StreamInfo>> {
        let Some(fmt) = Self::fmt_for(q) else {
            return Ok(None);
        };

        let info = self.musicinfo_text(&track.id).await?;
        let Some((s1, s2, size)) = Self::parse_sig(&info, fmt) else {
            return Ok(None); // 该曲目没有这个音质
        };

        let plain = Self::res_manage_plain(s1, s2);
        let cipher = crypt::encrypt(&plain);
        let body = BASE64.encode(&cipher).into_bytes();
        let resp = self
            .http
            .post_bytes(
                RES_MANAGE_HOST,
                &format!("http://{}/yl_res_manage.search", RES_MANAGE_HOST),
                body,
                "application/octet-stream",
            )
            .await?;

        let text: String = String::from_utf8_lossy(&resp)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
            .collect();
        let cipher_resp = BASE64.decode(text.as_bytes()).map_err(|e| {
            Error::Parse(format!(
                "资源管理响应 base64 解码失败({e}): {}",
                &text[..text.len().min(60)]
            ))
        })?;
        let plain_resp = String::from_utf8_lossy(&crypt::decrypt(&cipher_resp)).to_string();

        let (urls, _file_len) = Self::parse_res_manage(&plain_resp);
        let Some(url) = urls.into_iter().next() else {
            return Ok(None);
        };

        match quality::verify_url(&self.http, &url, track.duration_sec, q, "trackmedia").await {
            Ok(mut stream) => {
                // musicinfo 报的 SIZE 只作为字节数兜底；以探测结果为准。
                if stream.bytes.is_none() {
                    stream.bytes = Some(size);
                }
                Ok(Some(stream))
            }
            Err(_) => Ok(None),
        }
    }

    /// 歌手资料（头像 / 简介 / 基本资料），带 24h 缓存
    async fn artist_profile(&self, id: &str) -> SingerProfile {
        let id = id.trim();
        if id.is_empty() {
            return SingerProfile::default();
        }
        if let Some(hit) = self.profile_cache.get(&id.to_string()) {
            return hit;
        }

        let url = format!("https://www.kuwo.cn/singer_detail/{}", id);
        let mut profile = match self.http.get_text("www.kuwo.cn", &url).await {
            Ok(html) => parse_singer_page(&html),
            Err(_) => SingerProfile::default(),
        };

        if profile.pic.is_none() {
            profile.pic = self.artist_pic(id).await;
        }

        self.profile_cache.put(id.to_string(), profile.clone());
        profile
    }

    /// 兜底头像：酷我歌手写真接口（纯文本，每行一个 URL）
    async fn artist_pic(&self, id: &str) -> Option<String> {
        let url = format!(
            "http://artistpicserver.kuwo.cn/pic.web?type=big_artist_pic&pictype=url&content=list&id={}&name=&from=pc&json=0&size=1",
            id
        );
        let body = self
            .http
            .get_text("artistpicserver.kuwo.cn", &url)
            .await
            .ok()?;
        body.lines()
            .map(str::trim)
            .find(|l| l.starts_with("http"))
            .map(|l| l.replacen("http://", "https://", 1))
    }
}

#[async_trait::async_trait]
impl MusicProvider for KuwoProvider {
    fn id(&self) -> &'static str {
        "kuwo"
    }

    fn display_name(&self) -> &'static str {
        "酷我音乐"
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "kuwo".into(),
            name: "酷我音乐".into(),
            // 当前匿名公开链路可播的三档；高级档位在 UI 里单独说明
            qualities: vec![Quality::Standard, Quality::Super, Quality::Lossless],
        }
    }

    async fn search(&self, kw: &str, page: u32, limit: u32) -> Result<SearchPage> {
        let key = format!("{}|{}|{}", kw, page, limit);
        if let Some(hit) = self.search_cache.get(&key) {
            return Ok(hit);
        }
        let body = self
            .http
            .get_text(SEARCH_HOST, &Self::search_url(kw, page, limit))
            .await?;
        let parsed = Self::parse_search(&body, page, limit)?;
        self.search_cache.put(key, parsed.clone());
        Ok(parsed)
    }

    async fn resolve(&self, track: &Track, q: Quality) -> Result<StreamInfo> {
        self.resolve_with_fallback(track, q).await
    }

    async fn track_qualities(&self, track: &Track) -> Result<TrackQualities> {
        if !track.available_qualities.is_empty() {
            return Ok(TrackQualities {
                available: track.available_qualities.clone(),
                has_advanced: track.has_advanced,
                known: true,
            });
        }
        if let Some(hit) = self.quality_cache.get(&track.id) {
            return Ok(hit);
        }

        let info = self.musicinfo_text(&track.id).await?;
        let mut available = Vec::new();
        if Self::parse_sig(&info, "MP3128").is_some() {
            available.push(Quality::Standard);
        }
        if Self::parse_sig(&info, "MP3H").is_some() {
            available.push(Quality::Super);
        }
        if Self::parse_sig(&info, "ALFLAC").is_some() {
            available.push(Quality::Lossless);
        }
        let out = TrackQualities {
            available,
            has_advanced: quality::has_advanced_formats(&info) || track.has_advanced,
            known: true,
        };
        self.quality_cache.put(track.id.clone(), out.clone());
        Ok(out)
    }

    fn invalidate_resolve(&self, _id: &str) {
        self.resolve_cache.clear();
    }

    async fn detail(&self, track: &Track) -> Result<TrackDetail> {
        if let Some(hit) = self.detail_cache.get(&track.id) {
            return Ok(hit);
        }
        let body = self
            .http
            .get_text(DETAIL_HOST, &Self::detail_url(&track.id))
            .await?;
        let d = Self::parse_detail(&body, track.clone())?;
        self.detail_cache.put(track.id.clone(), d.clone());
        Ok(d)
    }

    async fn artist(&self, artist_id: &str, name: &str, page: u32) -> Result<ArtistPage> {
        let sp = self.search(name, page, 50).await?;
        // 头像/简介：PC 歌手页拿得到就优先用，拿不到退回搜索首条的专辑封面
        let profile = self.artist_profile(artist_id).await;
        let mut tracks: Vec<Track> = sp
            .items
            .iter()
            .filter(|t| t.artist_id == artist_id)
            .cloned()
            .collect();
        // 若过滤后为空（同名艺人或 id 不规范），退化为不过滤
        if tracks.is_empty() {
            tracks = sp.items.clone();
        }

        let mut albums: Vec<AlbumRef> = Vec::new();
        for t in &tracks {
            if t.album_id.is_empty() || t.album_id == "0" {
                continue;
            }
            if albums.iter().any(|a| a.id == t.album_id) {
                continue;
            }
            albums.push(AlbumRef {
                id: t.album_id.clone(),
                name: t.album.clone(),
                cover: t.cover.clone(),
                year: None,
            });
        }

        let pic = profile
            .pic
            .clone()
            .or_else(|| tracks.first().and_then(|t| t.cover.clone()));

        Ok(ArtistPage {
            artist_id: artist_id.to_string(),
            name: if name.is_empty() {
                profile.name.clone().unwrap_or_default()
            } else {
                name.to_string()
            },
            pic,
            desc: profile.bio.clone(),
            meta: profile.meta.clone(),
            albums,
            tracks,
        })
    }

    async fn album(&self, album_id: &str, name: &str, page: u32) -> Result<AlbumPage> {
        let sp = self.search(name, page, 50).await?;
        let mut tracks: Vec<Track> = sp
            .items
            .iter()
            .filter(|t| t.album_id == album_id)
            .cloned()
            .collect();
        if tracks.is_empty() {
            tracks = sp.items.clone();
        }
        let first = tracks.first().cloned().unwrap_or_default();
        Ok(AlbumPage {
            album_id: album_id.to_string(),
            name: if name.is_empty() {
                first.album.clone()
            } else {
                name.to_string()
            },
            artist: first.artist,
            cover: first.cover,
            tracks,
        })
    }

    async fn search_playlists(&self, kw: &str, page: u32, limit: u32) -> Result<PlaylistPage> {
        let key = format!("pl|{}|{}|{}", kw, page, limit);
        if let Some(hit) = self.search_cache.get(&key) {
            // 复用 TTL 缓存：把歌单页塞进同一张表不合适，这里只做请求级去重
            let _ = hit;
        }
        let body = self
            .http
            .get_text(SEARCH_HOST, &Self::search_playlist_url(kw, page, limit))
            .await?;
        Self::parse_playlist_search(&body, page)
    }

    async fn favorites(&self, uid: &str) -> Result<PlaylistDetail> {
        // 用户歌单里 type=MYFAVORITE 的那一张就是「我喜欢」
        let body = self
            .http
            .get_text(PLAYLIST_HOST, &Self::user_playlist_url(uid, 0, 200))
            .await?;
        let v: Value = serde_json::from_str(&body)
            .map_err(|e| Error::Parse(format!("用户歌单响应不是 JSON: {e}")))?;
        let fid = v
            .get("plist")
            .and_then(|p| p.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|it| kwjson::s(it, "type") == "MYFAVORITE")
                    .map(|it| kwjson::s(it, "id"))
            })
            .filter(|s| !s.is_empty());
        let Some(fid) = fid else {
            return Err(Error::NotFound("该账号没有「喜欢」歌单".into()));
        };
        self.playlist(&fid).await
    }

    async fn user_playlists(&self, uid: &str, limit: u32) -> Result<Vec<PlaylistRef>> {
        let body = self
            .http
            .get_text(PLAYLIST_HOST, &Self::user_playlist_url(uid, 0, limit))
            .await?;
        Self::parse_user_playlists(&body)
    }

    async fn playlist(&self, id: &str) -> Result<PlaylistDetail> {
        const RN: u32 = 100;
        const MAX_PAGES: u32 = 10; // 上限 1000 首，防止超大歌单拖死界面

        let mut all: Vec<Track> = Vec::new();
        let mut meta: Option<PlaylistRef> = None;
        let mut pn = 0u32;

        loop {
            let body = self
                .http
                .get_text(PLAYLIST_HOST, &Self::playlist_url(id, pn, RN))
                .await?;
            let d = Self::parse_playlist_detail(&body, id)?;
            let total = d.playlist.track_count;
            if meta.is_none() {
                meta = Some(d.playlist.clone());
            }
            if d.tracks.is_empty() {
                break;
            }
            all.extend(d.tracks);
            pn += 1;
            if all.len() as u32 >= total || pn >= MAX_PAGES {
                break;
            }
        }

        let mut playlist = meta.ok_or_else(|| Error::NotFound(format!("歌单 {id} 无内容")))?;
        playlist.track_count = if playlist.track_count == 0 {
            all.len() as u32
        } else {
            playlist.track_count
        };
        Ok(PlaylistDetail {
            playlist,
            tracks: all,
        })
    }

    fn features(&self) -> ProviderFeatures {
        // 酷我有账号级「喜欢」（type=MYFAVORITE 的歌单）
        ProviderFeatures {
            favorites: true,
            recommend: false,
        }
    }

    /// 酷我的登录能力如实声明：两条路都被服务端挡住，这里不放假登录框。
    fn auth_capabilities(&self) -> Vec<AuthCapability> {
        vec![
            AuthCapability::blocked(
                AuthMethod::Password,
                "酷我登录接口 pc.i.kuwo.cn/US_NEW/kuwo/login_kw 的响应是私有加密的固定密文包（40 字节，                 不含 Set-Cookie，标准 DES 与库内 Entrypt 算法均无法解开），无法判读登录结果。",
            ),
            AuthCapability::blocked(
                AuthMethod::Cookie,
                "「用户歌单」等账号接口需要 web 端 Secret 签名头，仅凭 cookie 无法通过。",
            ),
            AuthCapability::blocked(
                AuthMethod::QrCode,
                "扫码登录端点未在客户端中暴露为明文地址，暂无法接入。",
            ),
        ]
    }

    async fn my_playlists(&self, _session: &Session) -> Result<Vec<PlaylistRef>> {
        Err(Error::Unsupported(
            "酷我需要签名接口才能列出账号歌单；请改用「歌单搜索 / 按链接打开 / 本地收藏」。".into(),
        ))
    }
}

// ---------- 工具 ----------

fn first_nonempty(cands: &[String]) -> String {
    cands
        .iter()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// 会员专享曲的 PAY 特征值。实测 36 首样本 100% 对应「只给 11 秒占位片段」。
pub const PAY_VIP_ONLY: u32 = 16_711_935;

/// 用搜索结果的字段预判可用性，**不产生额外请求**。
///
/// * `PAY == 16711935` → 会员专享（实测 36/36 准确）
/// * `ONLINE == 0`     → 已下架
/// * 其余              → Unknown，等点击时再探测
///
/// 注意：不能用 `payInfo.down` 判断——实测它在受限曲上也会给 `'1111'`。
fn probe_availability_hint(v: &Value) -> Availability {
    if v.get("ONLINE").is_some() && kwjson::u(v, "ONLINE") == 0 {
        return Availability::Unavailable;
    }
    if kwjson::u(v, "PAY") == PAY_VIP_ONLY {
        return Availability::VipOnly;
    }
    Availability::Unknown
}

/// `*.kwcdn.kuwo.cn` 域族已下线（实测 NXDOMAIN），统一改回主域并升 https。
pub fn normalize_cdn(url: &str) -> String {
    let u = url.replace(".kwcdn.kuwo.cn", ".kuwo.cn");
    match u.strip_prefix("http://") {
        Some(rest) => format!("https://{}", rest),
        None => u,
    }
}

/// 最小化 urlencode（避免引依赖）；对 UTF-8 逐字节百分号编码
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_singer_page_profile() {
        let html = r#"<script>window.__NUXT__=(function(a,b){return {layout:"default",data:[{singerInfo:{birthday:"1979-01-18",country:"中国台湾",pic300:"https:\u002F\u002Fstar.kuwo.cn\u002Fstarheads\u002F300\u002Fx.jpg",info:"周杰伦，华语流行男歌手。&nbsp;\n基本资料",name:"周杰伦"}}]};})</script>"#;
        let p = parse_singer_page(html);
        assert_eq!(p.name.as_deref(), Some("周杰伦"));
        assert_eq!(
            p.pic.as_deref(),
            Some("https://star.kuwo.cn/starheads/300/x.jpg")
        );
        assert!(p.bio.unwrap_or_default().contains("华语流行男歌手"));
        assert!(p.meta.iter().any(|(k, v)| k == "生日" && v == "1979-01-18"));
        assert!(p.meta.iter().any(|(k, v)| k == "地区" && v == "中国台湾"));
    }

    const SEARCH_FIXTURE: &str = include_str!("../../../tests/fixtures/search_daoxiang.json");
    const ESCAPED_FIXTURE: &str = include_str!("../../../tests/fixtures/search_escaped.json");
    const RESOLVE_OK: &str = include_str!("../../../tests/fixtures/resolve_ok.json");
    const RESOLVE_PH: &str = include_str!("../../../tests/fixtures/resolve_placeholder.json");

    #[test]
    fn parses_search_fixture() {
        let page = KuwoProvider::parse_search(SEARCH_FIXTURE, 0, 3).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(page.total > 0);
        let t = &page.items[0];
        assert_eq!(t.provider, "kuwo");
        assert!(!t.id.is_empty());
        assert!(!t.title.is_empty());
        assert!(!t.artist.is_empty());
        assert!(t.duration_sec > 0);
        assert!(t
            .cover
            .as_deref()
            .unwrap_or("")
            .starts_with("https://img1.kuwo.cn/star/albumcover/"));
        assert!(
            page.items
                .iter()
                .any(|t| t.available_qualities.contains(&Quality::Lossless)),
            "搜索列表应从 MINFO 解析出无损能力"
        );
        assert!(
            page.items.iter().any(|t| t.has_advanced),
            "应识别出加密高级格式"
        );
    }

    #[test]
    fn cleans_escaped_artist() {
        let page = KuwoProvider::parse_search(ESCAPED_FIXTURE, 0, 3).unwrap();
        // 原始响应里艺人字段含 \\\\u0026，清洗后应变成 &
        assert!(
            page.items.iter().any(|t| t.artist.contains('&')),
            "期望至少一项艺人字段含 &，实际: {:?}",
            page.items.iter().map(|t| &t.artist).collect::<Vec<_>>()
        );
        // 不应残留 &nbsp; 或 \u 转义
        for t in &page.items {
            assert!(!t.artist.contains("&nbsp;"), "残留 &nbsp;: {}", t.artist);
            assert!(!t.artist.contains("\\u"), "残留转义: {}", t.artist);
            assert!(!t.title.contains("&nbsp;"), "残留 &nbsp;: {}", t.title);
        }
    }

    #[test]
    fn parses_resolve_ok() {
        let u = KuwoProvider::parse_resolve(RESOLVE_OK).unwrap();
        assert!(u.starts_with("https://"));
        assert!(!u.contains(PLACEHOLDER_PATH_ID));
    }

    #[test]
    fn detects_placeholder_by_url() {
        let u = KuwoProvider::parse_resolve(RESOLVE_PH).unwrap();
        let probe = ProbeInfo {
            status: 200,
            bytes: Some(PLACEHOLDER_BYTES),
            content_type: "audio/mpeg".into(),
            head: Vec::new(),
        };
        assert!(KuwoProvider::is_placeholder(&u, &probe, 223, 128));
    }

    #[test]
    fn detects_placeholder_by_size_even_without_url_id() {
        let probe = ProbeInfo {
            status: 200,
            bytes: Some(PLACEHOLDER_BYTES),
            content_type: "audio/mpeg".into(),
            head: Vec::new(),
        };
        assert!(KuwoProvider::is_placeholder(
            "https://example.com/x.mp3",
            &probe,
            200,
            128
        ));
    }

    #[test]
    fn detects_placeholder_by_duration_mismatch() {
        let probe = ProbeInfo {
            status: 200,
            bytes: Some(400_000),
            content_type: "audio/mpeg".into(),
            head: Vec::new(),
        };
        // 400KB @128k ≈ 25s，标称 251s → 片段
        assert!(KuwoProvider::is_placeholder(
            "https://example.com/x.mp3",
            &probe,
            251,
            128
        ));
    }

    #[test]
    fn real_file_not_flagged() {
        // 3,182,050 字节 @128k ≈ 198s，标称 198s
        let probe = ProbeInfo {
            status: 200,
            bytes: Some(3_182_050),
            content_type: "audio/mpeg".into(),
            head: Vec::new(),
        };
        assert!(!KuwoProvider::is_placeholder(
            "https://kw-bj.kuwo.cn/x/nf/resource/n2/25/44/97165461.mp3",
            &probe,
            198,
            128
        ));
    }

    #[test]
    fn rejects_failed_resolve() {
        assert!(KuwoProvider::parse_resolve("failed").is_err());
        assert!(KuwoProvider::parse_resolve(r#"{"code":200,"url":""}"#).is_err());
    }

    const PL_SEARCH: &str = include_str!("../../../tests/fixtures/playlist_search.json");
    const PL_DETAIL: &str = include_str!("../../../tests/fixtures/playlist_detail.json");

    #[test]
    fn parses_playlist_search() {
        let page = KuwoProvider::parse_playlist_search(PL_SEARCH, 0).unwrap();
        assert!(!page.items.is_empty(), "歌单搜索结果不应为空");
        assert!(page.total > 0);
        let p = &page.items[0];
        assert_eq!(p.provider, "kuwo");
        assert!(!p.id.is_empty(), "歌单 id 不能为空");
        assert!(!p.name.is_empty(), "歌单名不能为空");
        assert!(p.track_count > 0, "曲目数应 > 0");
        // 封面必须走可用域（kwcdn 已下线）
        if let Some(c) = &p.cover {
            assert!(!c.contains("kwcdn"), "封面域名未归一化: {}", c);
            assert!(c.starts_with("https://"), "封面应为 https: {}", c);
        }
    }

    #[test]
    fn parses_playlist_detail() {
        let d = KuwoProvider::parse_playlist_detail(PL_DETAIL, "2867496601").unwrap();
        assert_eq!(d.playlist.id, "2867496601");
        assert!(d.playlist.track_count > 0);
        assert_eq!(d.playlist.creator.as_deref(), Some("adbcfdc"));
        assert!(!d.tracks.is_empty(), "歌单曲目不应为空");
        let t = &d.tracks[0];
        assert!(!t.id.is_empty());
        assert!(!t.title.is_empty());
        assert!(!t.artist.is_empty());
        assert!(t.duration_sec > 0);
        // 曲目是否可播应保持 Unknown（真正可用性要靠 resolve 探测）
        assert_eq!(t.availability, Availability::Unknown);
        assert!(
            d.tracks
                .iter()
                .any(|x| x.available_qualities.contains(&Quality::Lossless)),
            "歌单里的 MINFO 也应解析出无损"
        );
    }

    #[test]
    fn playlist_failure_is_reported() {
        let r =
            KuwoProvider::parse_playlist_detail(r#"{"result":"fail","reason":"PARAM_ERROR"}"#, "1");
        assert!(matches!(r, Err(Error::NotFound(_))));
    }

    #[test]
    fn cdn_host_normalized() {
        assert_eq!(
            normalize_cdn("http://img1.kwcdn.kuwo.cn/star/userpl2015/a/700.jpg"),
            "https://img1.kuwo.cn/star/userpl2015/a/700.jpg"
        );
        // 已是主域则只升 https
        assert_eq!(
            normalize_cdn("http://img3.kuwo.cn/star/starheads/x.jpg"),
            "https://img3.kuwo.cn/star/starheads/x.jpg"
        );
        // https 原样保留
        assert_eq!(
            normalize_cdn("https://img1.kuwo.cn/x.jpg"),
            "https://img1.kuwo.cn/x.jpg"
        );
    }

    #[test]
    fn auth_capabilities_are_honest() {
        let p = KuwoProvider::new();
        let caps = p.auth_capabilities();
        assert!(!caps.is_empty());
        // 关键：不支持时必须给出原因，UI 才能解释而不是放个假登录框
        for c in &caps {
            if !c.supported {
                assert!(
                    c.reason.as_deref().map(|r| !r.is_empty()).unwrap_or(false),
                    "{:?} 声明为不支持但没给原因",
                    c.method
                );
            }
        }
    }

    #[test]
    fn vip_only_predicted_from_pay_without_extra_requests() {
        let page = KuwoProvider::parse_search(SEARCH_FIXTURE, 0, 3).unwrap();
        // fixture 里 440613(稻香原版) 的 PAY == 16711935
        let vip = page
            .items
            .iter()
            .find(|t| t.id == "440613")
            .expect("fixture 应含稻香");
        assert_eq!(vip.availability, Availability::VipOnly);

        // 非会员曲不得被误判为 VipOnly
        let others: Vec<_> = page.items.iter().filter(|t| t.id != "440613").collect();
        for t in others {
            assert_ne!(t.availability, Availability::VipOnly, "{} 被误判", t.id);
        }
    }

    #[test]
    fn urlencode_utf8() {
        assert_eq!(urlencode("稻香"), "%E7%A8%BB%E9%A6%99");
        assert_eq!(urlencode("a b"), "a%20b");
    }

    #[test]
    fn quality_br_mapping() {
        assert_eq!(Quality::Standard.nominal_kbps(), Some(128));
        assert_eq!(Quality::Super.nominal_kbps(), Some(320));
        assert_eq!(Quality::Lossless.nominal_kbps(), None);
        assert_eq!(Quality::Lossless.next_lower(), Some(Quality::Super));
        assert_eq!(Quality::Standard.next_lower(), None);
        assert_eq!(Quality::HiRes.next_lower(), None);
    }
}
