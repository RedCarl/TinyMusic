//! 网易云 → 酷我 的跨源匹配（曲库互补的兜底链路）。
//!
//! 场景：网易云登录后拿到每日推荐/歌单（元数据最全），但播放源是酷我；
//! 或者反过来，网易云取流失败时用酷我补位。
//!
//! ## 为什么不能只按标题搜
//!
//! 实测酷我 `search.kuwo.cn/r.s` 的排序严重偏向「翻唱 / DJ 版 / 抖音版」：
//!
//! ```text
//! 搜索「恋人 李荣浩」的前 5 条：
//!   恋人(2013版)|李健|168s
//!   恋人|李毅阳|240s
//!   恋人(Instrumental)|뎁트&慢热同学LEON|203s
//!   ...                            ← 李荣浩原版根本不在前 30 条里
//! ```
//!
//! 所以匹配必须做三件事：**规范化标题**（去版本后缀）、**校验歌手**、
//! **校验时长**，最后给一个置信度分，低于阈值宁可报「不可用」也不放错歌。
//!
//! 权重（与 `docs/音源扩展方案.md` 的 P2 一致）：
//! 标题 0.55 / 歌手 0.30 / 时长 0.15，阈值 0.75。

use crate::model::{MusicProvider, Track};
use crate::text::clean;

/// 命中结果
#[derive(Debug, Clone)]
pub struct Match {
    pub track: Track,
    pub score: f32,
    /// 标题/歌手/时长各自的得分，便于排查与展示
    pub parts: (f32, f32, f32),
}

/// 自动播放的置信度阈值
pub const ACCEPT_SCORE: f32 = 0.75;
/// 时长差超过这个值直接判为「不同版本」
const DURATION_HARD_REJECT_SEC: u32 = 25;

/// 标题规范化：去括号内容、去版本词、去标点与空白、小写。
pub fn normalize(raw: &str) -> String {
    let s = clean(raw).to_lowercase();

    // 1) 去掉括号段落（中英文括号都处理，可能嵌套一次）
    let mut stripped = String::with_capacity(s.len());
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '（' | '(' | '【' | '[' => depth += 1,
            '）' | ')' | '】' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => stripped.push(ch),
            _ => {}
        }
    }

    // 2) 去掉版本词与常见噪声
    let mut t = stripped;
    for noise in [
        "cover:", "cover：", "feat.", "feat ", "ft.", "live", "remix", "demo", "instrumental",
        "伴奏", "现场", "翻唱", "纯享版", "抖音版", "快手版", "加速版", "降速版", "dj版", "dj",
        "女声版", "男声版", "童声版", "片段", "铃声", "消音", "slowed", "reverb", "sped up",
    ] {
        t = t.replace(noise, "");
    }

    // 3) 去掉标点、空白、分隔符（只保留字母数字与汉字）
    t.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
}

/// 歌手列表：`A/B`、`A&B`、`A、B` 都要能拆开
fn artists(raw: &str) -> Vec<String> {
    clean(raw)
        .split(['/', '&', '、', ',', '，', ';', '；'])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn artist_score(want: &Track, got: &Track) -> f32 {
    let a = artists(&want.artist);
    let b = artists(&got.artist);
    if a.is_empty() || b.is_empty() {
        return 0.4; // 双方信息缺失时给中性分，交给标题/时长决定
    }
    // 任意一对命中即算（合作曲只要有一个对得上就是同一首）
    let mut best: f32 = 0.0;
    for x in &a {
        for y in &b {
            let s = if x == y {
                1.0
            } else if x.contains(y) || y.contains(x) {
                // 「李荣浩」vs「李荣浩/杨丞琳」这类包含关系只算部分匹配
                if x.chars().count().min(y.chars().count()) >= 2 {
                    0.7
                } else {
                    0.0
                }
            } else {
                0.0
            };
            if s > best {
                best = s;
            }
        }
    }
    best
}

fn duration_score(want_sec: u32, got_sec: u32) -> f32 {
    if want_sec == 0 || got_sec == 0 {
        return 0.5; // 未知时长不放行也不否决
    }
    let diff = want_sec.abs_diff(got_sec);
    if diff <= 5 {
        1.0
    } else if diff <= 15 {
        0.5
    } else {
        0.0
    }
}

/// 标题相似度：完全相同 1.0；包含 0.8；否则按字符二元组 Dice 系数打折。
fn title_score(want: &Track, got: &Track) -> f32 {
    let a = normalize(&want.title);
    let b = normalize(&got.title);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.contains(&b) || b.contains(&a) {
        return 0.8;
    }
    let dice = bigram_dice(&a, &b);
    if dice >= 0.6 {
        dice * 0.8
    } else {
        0.0
    }
}

/// 二元组 Dice 系数，处理「甲乙丙丁」vs「甲乙丙丁（你我怎么两清）」之外的改写
fn bigram_dice(a: &str, b: &str) -> f32 {
    let ga: Vec<char> = a.chars().collect();
    let gb: Vec<char> = b.chars().collect();
    if ga.len() < 2 || gb.len() < 2 {
        return if ga == gb { 1.0 } else { 0.0 };
    }
    let grams = |s: &[char]| -> Vec<String> {
        s.windows(2)
            .map(|w| w.iter().collect::<String>())
            .collect()
    };
    let mut x = grams(&ga);
    let y = grams(&gb);
    let total = x.len() + y.len();
    if total == 0 {
        return 0.0;
    }
    let mut hit = 0usize;
    for g in &y {
        if let Some(pos) = x.iter().position(|v| v == g) {
            x.remove(pos);
            hit += 1;
        }
    }
    (2.0 * hit as f32) / total as f32
}

/// 给一对曲目打分。返回 0..1。
pub fn score(want: &Track, got: &Track) -> f32 {
    let t = title_score(want, got);
    if t == 0.0 {
        return 0.0;
    }
    let (a, d) = (artist_score(want, got), duration_score(want.duration_sec, got.duration_sec));

    // 时长差过大（两边都已知）直接判为不同版本，避免 Live/剪辑版串歌
    if want.duration_sec > 0
        && got.duration_sec > 0
        && want.duration_sec.abs_diff(got.duration_sec) > DURATION_HARD_REJECT_SEC
    {
        return 0.0;
    }

    let mut s = t * 0.55 + a * 0.30 + d * 0.15;

    // 候选是翻唱/伴奏而目标不是：降权
    let goal = clean(&want.title).to_lowercase();
    let cand = clean(&got.title).to_lowercase();
    let noisy = ["伴奏", "cover", "现场", "live", "翻唱", "dj", "抖音", "铃声", "消音", "片段"];
    let want_noisy = noisy.iter().any(|n| goal.contains(n));
    if !want_noisy && noisy.iter().any(|n| cand.contains(n)) {
        s -= 0.20;
    }
    s.clamp(0.0, 1.0)
}

/// 在酷我里找与 `want` 对应的一首。
///
/// `kuwo.search` 的排序不可靠，所以这里跑两轮查询、取全部候选里分最高的一首。
pub async fn find_in_kuwo(kuwo: &dyn MusicProvider, want: &Track) -> Option<Match> {
    let mut queries = vec![format!("{} {}", want.title, want.artist)];
    queries.push(want.title.clone());
    if let Some(first) = artists(&want.artist).first() {
        queries.push(format!("{} {}", want.title, first));
    }

    let mut best: Option<Match> = None;
    for q in queries {
        let page = match kuwo.search(&q, 0, 30).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        for cand in page.items {
            let s = score(want, &cand);
            let better = best.as_ref().map(|b| s > b.score).unwrap_or(true);
            if better && s > 0.05 {
                best = Some(Match {
                    track: cand.clone(),
                    score: s,
                    parts: (
                        title_score(want, &cand),
                        artist_score(want, &cand),
                        duration_score(want.duration_sec, cand.duration_sec),
                    ),
                });
            }
        }
        // 已经足够可信，不必再换关键词
        if best.as_ref().map(|b| b.score >= 0.9).unwrap_or(false) {
            break;
        }
    }
    best.filter(|m| m.score >= ACCEPT_SCORE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;

    fn t(title: &str, artist: &str, dur: u32) -> Track {
        Track {
            provider: "test".into(),
            id: "1".into(),
            title: title.into(),
            artist: artist.into(),
            duration_sec: dur,
            ..Default::default()
        }
    }

    #[test]
    fn normalize_strips_versions() {
        assert_eq!(normalize("恋人 (Live)"), "恋人");
        assert_eq!(normalize("甲乙丙丁（你我怎么两清）"), "甲乙丙丁");
        assert_eq!(normalize("天龙八部之宿敌&nbsp;(cover: 许嵩)"), "天龙八部之宿敌");
        assert_eq!(normalize("海屿你 0.8x"), "海屿你08x");
    }

    #[test]
    fn exact_match_scores_high() {
        let a = t("晴天", "周杰伦", 269);
        let b = t("晴天", "周杰伦", 269);
        assert!(score(&a, &b) > 0.95);
    }

    #[test]
    fn cover_is_penalised() {
        let want = t("天龙八部之宿敌", "许嵩", 256);
        let cover = t("天龙八部之宿敌 (cover: 许嵩)", "叶洛洛", 256);
        let s = score(&want, &cover);
        assert!(s < ACCEPT_SCORE, "翻唱不该过阈值，实际 {s}");
    }

    #[test]
    fn wrong_artist_fails() {
        let want = t("恋人", "李荣浩", 240);
        let other = t("恋人", "李健", 251);
        assert!(score(&want, &other) < ACCEPT_SCORE);
    }

    #[test]
    fn duration_mismatch_fails() {
        let want = t("稻香", "周杰伦", 223);
        let live = t("稻香", "周杰伦", 300);
        assert_eq!(score(&want, &live), 0.0);
    }

    #[test]
    fn amp_entities_do_not_break_artist() {
        let want = t("罗生门", "梨冻紧/Wiz_H张子豪", 200);
        let got = t("罗生门 (Follow)", "梨冻紧\\u0026Wiz_H张子豪", 200);
        assert!(score(&want, &got) >= ACCEPT_SCORE, "{}", score(&want, &got));
    }

    #[test]
    fn dice_handles_rewrites() {
        assert!(bigram_dice("晴天", "晴天") == 1.0);
        // 短标题对长标题：Dice 天然偏低，靠前面的「包含」分支兜住
        let d = bigram_dice("晴天", "晴天2026版");
        assert!((0.2..0.5).contains(&d), "dice={d}");
        let want = t("晴天", "周杰伦", 269);
        let got = t("晴天 2026版", "周杰伦", 269);
        assert!(title_score(&want, &got) >= 0.8);
    }
}
