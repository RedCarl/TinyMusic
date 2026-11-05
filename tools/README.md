# 酷我取链抓包工具

目标：在**能正常播放付费歌曲**的那台 Windows 机器上，抓到客户端真正发出的取链请求，
用来确认破解版到底走的是哪个接口、带了什么凭据。

## 为什么不用 Fiddler / Charles

因为**根本不需要**。老客户端取链用的是明文 HTTP：

```
http://antiserver.kuwo.cn/anti.s?...
```

（这是在 `PlayerCore.dll` 里硬编码的，已确认）。明文就说明不需要装根证书、不需要 MITM，
只要把域名指到本机就能看全。

## 用法

1. 把 `kwcap.ps1` 和 `开始抓包.bat` 放到同一目录
2. 双击 `开始抓包.bat`（会自己弹 UAC 要管理员权限）
3. 窗口显示「抓包已启动」后，打开酷我客户端
4. **放一首你确认是付费/会员的歌**，建议选没播放过的（有缓存就不会重新请求）
5. 播放 10 秒左右，回到窗口按 `Ctrl+C`
6. 把同目录的 `kwcap.log` 发回来

脚本退出时会自动还原 `hosts`。如果异常退出没还原，执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\kwcap.ps1 -Restore
```

## 它会记录什么

每条请求的完整 URL、请求体，以及响应的前 2KB：

```
[09:31:02] >>> GET http://antiserver.kuwo.cn/anti.s?format=aac&type=convert_url&rid=MUSIC_440613&...
[09:31:02]     HTTP/1.1 200 OK
[09:31:02]     RESP: https://kw-bj.kuwo.cn/xxxx/nf/resource/n1/69/32/588957081.mp3
[09:31:02]     ----
```

关键看 `RESP` 那段：

| 返回值 | 含义 |
|---|---|
| 含 `588957081` | 还是占位文件（服务器拒绝） |
| 其他 `nf/resource/...` 路径 | **真实音频**，说明确实拿到了整曲 |
| 完全不同的域名 | 走了别的接口，需要顺着查 |

## 抓不到的排查

**一条都没记录**
- 客户端没重新请求：换一首没播过的歌，或清空 `C:\ProgramData\kuwodata` 后重试
- 客户端用了自定义代理：检查客户端的网络设置，关掉代理

**只有占位文件出来**
- 说明这台机器上匿名身份确实拿不到整曲，那结论就和我在 macOS 上测的一致
- 这种情况下请确认：你听的那首歌**是不是真的完整**？还是只有十几秒或音质很低

**80 端口被占用**
- IIS / 其它 Web 服务占了。先停掉那个服务再跑，否则 hosts 指过来会连到别的程序

## 抓到的请求发我之后

我能判断三件事：

1. **接口**：是不是 `anti.s`，还是 `getVipSamInfoForVirtualUser` 那条虚拟用户链路
2. **凭据**：`loginid` / `vuid` / `sid` / `token` 填的是什么，是不是内嵌了某个账号
3. **音质**：返回的是哪一档，路径 `n1`/`n2` 对应什么

然后才能在 TinyMusic 里复刻同样的请求。

## 拦截范围

只拦这七个明文 HTTP 域名（HTTPS 域名不碰，因为那需要装证书）：

```
antiserver.kuwo.cn    取链（最关键）
search.kuwo.cn        搜索
nplserver.kuwo.cn     歌单
pc.i.kuwo.cn          登录 / 虚拟用户
mobi.kuwo.cn          移动端取链
nmobi.kuwo.cn
loginserver.kuwo.cn
```

CDN 域名（`kw-bj.kuwo.cn` 等）**故意不拦**——那是音频文件下载，拦了只会让日志被
几 MB 的二进制塞满，对定位问题没有帮助。

## 关于编码（踩过的坑）

Windows 上这个工具最大的坑不是抓包，是**文件编码**：

| 文件 | 要求 | 原因 |
|---|---|---|
| `开始抓包.bat` | **纯 ASCII** | cmd 按系统 ANSI(GBK) 读 .bat，UTF-8 中文会变乱码，乱码里混进 `&`/`\|` 就被当成命令执行 |
| `kwcap.ps1` | **UTF-8 with BOM** | Windows PowerShell 5.1 靠 BOM 判定编码，没有 BOM 就按 ANSI 读，中文全乱 |

另外一个 PowerShell 5.1 的坑：**不能在 `param()` 默认值里用 `$PSScriptRoot`**，
那时它还是空串，`Join-Path` 会直接报
`Cannot bind argument to parameter 'Path' because it is an empty string`。
脚本里改成在体内计算，并做了三重兜底。

## hosts 恢复：四重保险

改 hosts 是有风险的操作（改坏了酷我、浏览器都可能受影响），所以做了四层：

1. **`finally` 块内联还原** —— Ctrl+C 时走这条，不依赖任何函数是否还在作用域
2. **`PowerShell.Exiting` 引擎事件** —— 关窗口时兜底（逻辑内联，不调脚本函数）
3. **启动自愈** —— 下次运行时如果发现上次残留的 `hosts.bak`，先还原再继续
4. **`-Restore` 手动恢复** —— `kwcap.ps1 -Restore`

手动恢复命令：

```powershell
powershell -ExecutionPolicy Bypass -File .\kwcap.ps1 -Restore
```

脚本目录同时保留 `hosts.bak`，实在不行把它复制回
`C:\Windows\System32\drivers\etc\hosts` 即可。

## 抓包已经解出来的东西（musicinfo 包格式）

客户端不走 `anti.s` 的一个接口：

```
http://search.kuwo.cn/r.s?stype=musicinfo&itemset=music_2014&alflac=1&pcmp4=1&ids=MUSIC_<rid>
```

**响应是自定义二进制包**，格式：

```
偏移 0  : u32 LE  解压后长度
偏移 4  : u32 LE  压缩数据长度 (= 总长度 - 8)
偏移 8  : zlib 流 (78 9c ...)
```

解压后是纯文本，形如：

```
MUSICRID=MUSIC_440613
FORMATS=MNAC440|DTSX|...|MP3H|MP3128|ALFLAC|
MP3128=S1:268512270|S2:3799866485|SIZE:3576668|BT:128
MP3H=S1:1019510456|S2:1520348847|SIZE:8940938|BT:320
ALFLAC=S1:475949356|S2:219548347|SIZE:26012257
```

字段含义（已用免费曲对照验证）：

| 字段 | 含义 |
|---|---|
| `S1` | **CDN 上的文件名**（即 URL 末尾的 `{fid}.mp3`） |
| `S2` | 决定路径分片：`d1 = S2 % 100`，`d2 = (S2 // 100) % 100` |
| `SIZE` | 文件精确字节数 |
| `BT` | 码率 |

验证样本：

```
fid=97165461  S2=2165584425  ->  URL 路径 n2/25/44/97165461.mp3
  2165584425 % 100        = 25  ✓
  (2165584425 // 100) % 100 = 44  ✓
```

CDN URL 结构：

```
https://{host}/{32位token}/{8位unix时间戳十六进制}/nf/resource/{n}/{d1}/{d2}/{fid}.mp3
```

**注意：token 与路径绑定** —— 拿 A 文件的 token 去请求 B 文件会直接 403。
所以签名只能由服务端签发，客户端自己拼不出来。这就是为什么要抓包看它到底从哪拿签名。

## 已验证

脚本的核心逻辑（HTTP 请求解析、转发、响应回写）已在 macOS 上用 pwsh 实测通过：

- 请求解析：`Method / Path / Host / UA` 全部正确
- 转发：`Host` 保留、`Accept-Encoding` 主动剥离（强迫服务端返回明文，便于记录）、
  强制 `Connection: close`
- 响应完整回传给客户端
