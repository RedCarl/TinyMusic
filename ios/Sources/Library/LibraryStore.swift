import Foundation
import Observation

@MainActor
@Observable
final class LibraryStore {
    private(set) var playlists: [Playlist] = []
    private(set) var tracksByPlaylist: [String: [Track]] = [:]
    private(set) var favorites: [FavoriteTrack] = []
    /// 「喜欢」的快速索引。`isFavorite` 在每行每趟 body 都会调用，
    /// 而 `favoriteKey` 每次要分配 3 个字符串（实测约 2 µs），遍历收藏列表就是 O(F)。
    @ObservationIgnored private var favoriteKeyIndex: Set<String> = []
    @ObservationIgnored private var favoriteIDIndex: Set<String> = []
    private(set) var loadingPlaylists = false
    private(set) var loadingTracks = false
    private(set) var loadingFavorites = false
    private(set) var lastLoadFailed = false
    private(set) var error: String?
    private(set) var favoriteSyncWarning: String?

    private let backend: RustBackend

    init(backend: RustBackend = .shared) {
        self.backend = backend
    }

    func loadPlaylists() async {
        loadingPlaylists = true
        defer { loadingPlaylists = false }
        do {
            let result: [Playlist] = try await backend.call("s3_playlists")
            playlists = result
            lastLoadFailed = false
            error = nil
        } catch {
            lastLoadFailed = true
            self.error = error.localizedDescription
        }
    }

    /// 全局刷新：清掉曲目缓存并同时更新歌单和喜欢，避免进入详情页还是旧数据。
    func refreshAll() async {
        tracksByPlaylist.removeAll()
        await loadPlaylists()
        await pullFavorites()
    }

    func invalidateTracks() {
        tracksByPlaylist.removeAll()
    }

    func loadTracks(for playlist: Playlist) async {
        loadingTracks = true
        defer { loadingTracks = false }
        do {
            let result: [Track] = try await backend.call("s3_playlist_tracks", [
                "name": .string(playlist.name),
                "root": .bool(playlist.root),
            ])
            tracksByPlaylist[playlist.name] = result
            lastLoadFailed = false
            error = nil
        } catch {
            lastLoadFailed = true
            self.error = error.localizedDescription
        }
    }

    func tracks(for playlist: Playlist) -> [Track] {
        tracksByPlaylist[playlist.name] ?? []
    }

    func pullFavorites() async {
        loadingFavorites = true
        defer { loadingFavorites = false }
        do {
            let result: [FavoriteTrack] = try await backend.call("favorites_pull")
            var seen = Set<String>()
            // S3-only 客户端只显示 S3 收藏，避免旧数据库里的网易云/酷我条目混入。
            favorites = result.filter { favorite in
                guard favorite.provider == "s3" else { return false }
                return seen.insert(Self.favoriteKey(title: favorite.title, artist: favorite.artist)).inserted
            }
            rebuildFavoriteIndex()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    func isFavorite(_ track: Track) -> Bool {
        // 先读一次 `favorites` 登记 Observation 依赖：心形图标必须跟着收藏列表变化刷新。
        // 索引本身是 @ObservationIgnored，读它不会建立依赖。
        _ = favorites.count
        if favoriteIDIndex.contains(track.key) { return true }
        return favoriteKeyIndex.contains(Self.favoriteKey(title: track.title, artist: track.artist))
    }

    private func rebuildFavoriteIndex() {
        favoriteIDIndex = Set(favorites.map(\.id))
        favoriteKeyIndex = Set(favorites.map { Self.favoriteKey(title: $0.title, artist: $0.artist) })
    }

    @discardableResult
    func addFavorite(_ track: Track) async -> Bool {
        guard !isFavorite(track) else { return true }
        do {
            let result: FavoriteResult = try await backend.call("favorites_add", [
                "key": .string(track.key),
                "title": .string(track.title),
                "artist": .string(track.artist),
            ])
            // 收藏写进网盘 favorites.json；写不进去就不算收藏，
            // 否则下次拉取会把本地这条抹掉，喜欢里凭空少一首歌。
            guard result.synced else {
                let reason = (result.error?.isEmpty == false ? result.error! : L("收藏失败：没写进网盘"))
                favoriteSyncWarning = reason
                error = reason
                return false
            }
            favorites.append(FavoriteTrack(
                provider: "s3",
                id: result.key ?? track.key,
                title: track.title,
                artist: track.artist,
                cover: track.cover
            ))
            rebuildFavoriteIndex()
            favoriteSyncWarning = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func removeFavorite(_ track: Track) async -> Bool {
        let key = Self.favoriteKey(title: track.title, artist: track.artist)
        let ids = Set(favorites.filter {
            $0.id == track.key || Self.favoriteKey(title: $0.title, artist: $0.artist) == key
        }.map(\.id))
        do {
            for id in ids {
                let _: FavoriteResult = try await backend.call("favorites_remove", [
                    "key": .string(id),
                ])
            }
            favorites.removeAll {
                $0.id == track.key || Self.favoriteKey(title: $0.title, artist: $0.artist) == key
            }
            rebuildFavoriteIndex()
            favoriteSyncWarning = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func createPlaylist(named raw: String) async -> Bool {
        let name = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else {
            error = L("歌单名不能为空")
            return false
        }
        do {
            let created: String = try await backend.call("playlist_create", ["name": .string(name)])
            await loadPlaylists()
            tracksByPlaylist[created] = []
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func renamePlaylist(_ playlist: Playlist, to raw: String) async -> Bool {
        guard !playlist.root else {
            error = L("「未分类」是根目录，不能重命名")
            return false
        }
        let name = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else {
            error = L("歌单名不能为空")
            return false
        }
        do {
            let renamed: String = try await backend.call("playlist_rename", [
                "old": .string(playlist.name),
                "new": .string(name),
            ])
            let cached = tracksByPlaylist.removeValue(forKey: playlist.name)
            tracksByPlaylist[renamed] = cached
            await loadPlaylists()
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func deletePlaylist(_ playlist: Playlist) async -> Bool {
        guard !playlist.root else {
            error = L("「未分类」是根目录，不能删除")
            return false
        }
        do {
            let _: OKResponse = try await backend.call("playlist_delete", ["name": .string(playlist.name)])
            tracksByPlaylist.removeValue(forKey: playlist.name)
            await loadPlaylists()
            await pullFavorites()
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func deleteTrack(_ track: Track) async -> Bool {
        do {
            let _: OKResponse = try await backend.call("track_delete", ["key": .string(track.key)])
            for name in Array(tracksByPlaylist.keys) {
                guard let tracks = tracksByPlaylist[name] else { continue }
                tracksByPlaylist[name] = tracks.filter { $0.key != track.key }
            }
            favorites.removeAll { $0.id == track.key }
            rebuildFavoriteIndex()
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func moveTrack(_ track: Track, to target: Playlist) async -> Bool {
        guard !target.root else {
            error = L("根目录不能作为目标歌单")
            return false
        }
        do {
            let _: OKResponse = try await backend.call("track_move", [
                "key": .string(track.key),
                "target": .string(target.name),
            ])
            for name in Array(tracksByPlaylist.keys) {
                guard let tracks = tracksByPlaylist[name] else { continue }
                tracksByPlaylist[name] = tracks.filter { $0.key != track.key }
            }
            tracksByPlaylist.removeValue(forKey: target.name)
            await loadPlaylists()
            await loadTracks(for: target)
            // 移进/移出「喜欢」目录会改变收藏，跟着刷一次
            await pullFavorites()
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    private static func favoriteKey(title: String, artist: String) -> String {
        func normalize(_ value: String) -> String {
            String(value.lowercased().filter { $0.isLetter || $0.isNumber })
        }
        return "\(normalize(title))|\(normalize(artist))"
    }

    func clear() {
        playlists = []
        tracksByPlaylist = [:]
        favorites = []
        rebuildFavoriteIndex()
        lastLoadFailed = false
        error = nil
        favoriteSyncWarning = nil
    }
}
