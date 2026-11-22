//! 命令分发：`dispatch(cmd, args)` 是桌面（tm_invoke）与 mobile（HTTP）的共同入口。
//!
//! 参数名与前端一致（camelCase）；返回值沿用各类型自身的 serde 字段名。
//! 只在桌面壳里实现的命令（登录窗口、托盘、更新）不在这里。

use serde_json::Value;

use crate::*;

pub async fn dispatch(
    state: &AppState,
    cmd: &str,
    args: Value,
    sink: Option<ProgressFn>,
    s3_sink: Option<S3FixFn>,
) -> Result<Value, String> {
    match cmd {
        "get_providers" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = get_providers(state);
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "search" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                kw: String,
                page: u32,
                limit: u32,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = search(state, a.provider, a.kw, a.page, a.limit).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "resolve" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
                quality: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = resolve(state, a.track, a.quality).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "track_qualities" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = track_qualities(state, a.track).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "track_detail" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                track: Track,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = track_detail(state, a.provider, a.track).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "artist" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                artist_id: String,
                name: String,
                page: u32,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = artist(state, a.provider, a.artist_id, a.name, a.page).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "album" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                album_id: String,
                name: String,
                page: u32,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = album(state, a.provider, a.album_id, a.name, a.page).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "player_open" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
                quality: String,
                force_refresh: Option<bool>,
                allow_local: Option<bool>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = player_open(state, a.track, a.quality, a.force_refresh, a.allow_local).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_start" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
                quality: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: ProgressFn = sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = download_start(state, a.track, a.quality, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_list" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_list(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_clear" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_clear(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_failures" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_failures(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_failure_dismiss" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                id: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_failure_dismiss(state, a.id)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_failures_clear" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_failures_clear(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_retry_failed" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                ids: Vec<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = download_retry_failed(state, a.ids).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorite_add" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorite_add(state, a.track)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorite_remove" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                id: String,
                /// 收藏存在「喜欢」目录里，key 跟歌单里的原始 key 不同：带上标题/歌手才能认出来
                #[serde(default)]
                title: Option<String>,
                #[serde(default)]
                artist: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorite_remove(state, a.provider, a.id, a.title, a.artist).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorite_list" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorite_list(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorite_check" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                id: String,
                /// 标题/歌手：收藏存在「喜欢」目录里，key 跟歌单里的原始 key 不一样，
                /// 前端把这两项带上才能认出「同一首歌」
                #[serde(default)]
                title: Option<String>,
                #[serde(default)]
                artist: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorite_check(state, a.provider, a.id, a.title, a.artist)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorites_dedupe_scan" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorites_dedupe_scan(state).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorites_dedupe_purge" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                keys: Vec<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorites_dedupe_purge(state, a.keys).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorites_pull" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorites_pull(state).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "settings_get" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = settings_get(state);
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "settings_set" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                settings: Settings,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = settings_set(state, a.settings)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "quality_set" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                quality: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = quality_set(state, a.quality)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "quality_capabilities" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = quality_capabilities();
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "search_playlists" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                kw: String,
                page: u32,
                limit: u32,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = search_playlists(state, a.provider, a.kw, a.page, a.limit).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "playlist" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                id: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = playlist(state, a.provider, a.id).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "user_playlists" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                uid: String,
                limit: Option<u32>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = user_playlists(state, a.provider, a.uid, a.limit).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "favorites" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                uid: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = favorites(state, a.provider, a.uid).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "download_batch" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                tracks: Vec<Track>,
                quality: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: ProgressFn = sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = download_batch(state, a.tracks, a.quality, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "saved_playlist_add" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                playlist: PlaylistRef,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = saved_playlist_add(state, a.playlist)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "saved_playlist_remove" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                id: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = saved_playlist_remove(state, a.provider, a.id)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "saved_playlist_list" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = saved_playlist_list(state)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "saved_playlist_check" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                id: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = saved_playlist_check(state, a.provider, a.id)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "provider_features" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = provider_features(state, a.provider)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "auth_capabilities" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = auth_capabilities(state, a.provider)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "login" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                method: String,
                user: Option<String>,
                password: Option<String>,
                cookie: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = login(state, a.provider, a.method, a.user, a.password, a.cookie).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "logout" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = logout(state, a.provider).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "session_get" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = session_get(state, a.provider)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "my_playlists" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = my_playlists(state, a.provider).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "recommend_songs" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                limit: u32,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = recommend_songs(state, a.provider, a.limit).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "recommend_playlists" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = recommend_playlists(state, a.provider).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_test" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_test(state).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlists" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {}
            #[allow(unused_variables)]
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlists(state).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlist_tracks" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                name: String,
                root: Option<bool>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlist_tracks(state, a.name, a.root).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlist_purge" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                name: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlist_purge(state, a.name).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlist_rename" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                old: String,
                new: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlist_rename(state, a.old, a.new).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_cover" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                key: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_cover(state, a.key).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_track_delete" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                key: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_track_delete(state, a.key).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlists_reorder" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                names: Vec<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlists_reorder(state, a.names)?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_playlist_create" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                name: String,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_playlist_create(state, a.name).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_download" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                track: Track,
                quality: Option<String>,
                playlist: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: ProgressFn = sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = s3_download(state, a.track, a.quality, a.playlist, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_download_batch" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                tracks: Vec<Track>,
                quality: Option<String>,
                playlist: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: ProgressFn = sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = s3_download_batch(state, a.tracks, a.quality, a.playlist, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_integrity_check" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                playlist: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let out = s3_integrity_check(state, a.playlist).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_integrity_fix" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                playlist: Option<String>,
                ids: Vec<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: S3FixFn = s3_sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = s3_integrity_fix(state, a.playlist, a.ids, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        "s3_import_playlist" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct A {
                provider: String,
                playlist_id: String,
                name: String,
                target: Option<String>,
            }
            let a: A = serde_json::from_value(args.clone()).map_err(|e| format!("参数错误: {e}"))?;
            let cb: ProgressFn = sink.ok_or_else(|| "缺少进度回调".to_string())?;
            let out = s3_import_playlist(state, a.provider, a.playlist_id, a.name, a.target, cb).await?;
            serde_json::to_value(out).map_err(|e| e.to_string())
        }
        other => Err(format!("未知命令: {other}")),
    }
}
