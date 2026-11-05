<#
    kwcap.ps1 —— 酷我客户端取链接口抓包（零依赖，仅用 Windows 自带 PowerShell）

    原理
    ----
    酷我的取链接口 antiserver.kuwo.cn 走的是**明文 HTTP**，所以不需要装证书、不需要 MITM。
    脚本把下面这些域名临时指向 127.0.0.1，在本机 80 端口做反向代理：
    收到请求 → 完整记录 → 转发给真实服务器 → 记录响应 → 原样回给客户端。
    客户端感知不到任何区别。

    用法
    ----
    1. 右键「以管理员身份运行 PowerShell」（必须管理员，否则改不了 hosts）
    2. 进入脚本所在目录，执行：
           powershell -ExecutionPolicy Bypass -File .\kwcap.ps1
    3. 看到「抓包已启动」后，打开酷我，**播放一首你能确认是付费/会员的歌**
       建议先清一下客户端缓存，或者换一首没放过的，确保它会重新请求
    4. 播放 10 秒左右，回到 PowerShell 按 Ctrl+C 停止
    5. 把生成的 kwcap.log 发给我（同目录下）

    脚本退出时会自动把 hosts 恢复原样。若异常退出没恢复，
    执行 .\kwcap.ps1 -Restore 即可还原。
#>

[CmdletBinding()]
param(
    [switch]$Restore,
    [int]$Port = 80,
    [string]$LogFile
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# 定位脚本所在目录
#
# 注意：**不要**在 param() 默认值里用 $PSScriptRoot。
# Windows PowerShell 5.1 在绑定参数时它还是空串，Join-Path 会直接报
# "Cannot bind argument to parameter 'Path' because it is an empty string"。
# 必须放到脚本体里算，并且多准备几条兜底路径。
# ---------------------------------------------------------------------------
$ScriptDir = $PSScriptRoot
if ([string]::IsNullOrEmpty($ScriptDir) -and -not [string]::IsNullOrEmpty($PSCommandPath)) {
    $ScriptDir = Split-Path -Parent -Path $PSCommandPath
}
if ([string]::IsNullOrEmpty($ScriptDir) -and -not [string]::IsNullOrEmpty($MyInvocation.MyCommand.Path)) {
    $ScriptDir = Split-Path -Parent -Path $MyInvocation.MyCommand.Path
}
if ([string]::IsNullOrEmpty($ScriptDir)) {
    $ScriptDir = (Get-Location).Path
}
if ([string]::IsNullOrEmpty($LogFile)) {
    $LogFile = Join-Path -Path $ScriptDir -ChildPath 'kwcap.log'
}

if ($PSVersionTable.PSVersion.Major -lt 5) {
    Write-Host "[!] 需要 PowerShell 5.0 及以上" -ForegroundColor Red
    exit 1
}
$HostsFile = "$env:WINDIR\System32\drivers\etc\hosts"
$BackupFile = Join-Path -Path $ScriptDir -ChildPath 'hosts.bak'

# 只拦截明文 HTTP 的接口域名。HTTPS 域名不碰（无法在不装证书的情况下解密）。
$Targets = @(
    # ---- 核心接口 ----
    'antiserver.kuwo.cn',       # 取链 anti.s
    'search.kuwo.cn',           # 搜索 / musicinfo 签名包
    'searchv2.kuwo.cn',
    'nplserver.kuwo.cn',        # 歌单
    'hplserver.kuwo.cn',
    'cldserver.kuwo.cn',        # 收藏/云
    'checkdcserver.kuwo.cn',
    'musicpay.kuwo.cn',         # 付费/会员
    'payrecall.kuwo.cn',
    'vip.kuwo.cn',
    'vip1.kuwo.cn',
    'deliver.kuwo.cn',          # 投递
    'log.deliver.kuwo.cn',
    'multi.kuwo.cn',
    'pc.i.kuwo.cn',             # 登录 / 虚拟用户
    'loginserver.kuwo.cn',
    'mobi.kuwo.cn',             # 移动端取链
    'nmobi.kuwo.cn',
    'album.kuwo.cn',
    'shouji.kuwo.cn',
    'i.kuwo.cn',
    'artistpicserver.kuwo.cn',
    # ---- 音频 CDN 分片 ----
    'kw-bj.kuwo.cn',
    'kw-lw.kuwo.cn',
    'kw-er.kuwo.cn',
    'kw-lv.kuwo.cn',
    'kw-gz.kuwo.cn'
)

$Marker = '# === kwcap (temporary) ==='

function Write-Log {
    param([string]$Text)
    $line = "[{0:HH:mm:ss}] {1}" -f (Get-Date), $Text
    Write-Host $line
    Add-Content -LiteralPath $LogFile -Value $line -Encoding UTF8 -ErrorAction SilentlyContinue
}

function Restore-Hosts {
    if (Test-Path $BackupFile) {
        Copy-Item -LiteralPath $BackupFile -Destination $HostsFile -Force
        Remove-Item -LiteralPath $BackupFile -Force
        Write-Host "[+] hosts 已还原" -ForegroundColor Green
    } else {
        # 没有备份时，按标记行清除
        $content = Get-Content -LiteralPath $HostsFile -ErrorAction SilentlyContinue
        if ($content -match [regex]::Escape($Marker)) {
            $kept = $content | Where-Object { $_ -notmatch [regex]::Escape($Marker) -and $_ -notmatch '\bkwcap\b' }
            Set-Content -LiteralPath $HostsFile -Value $kept -Encoding ASCII
            Write-Host "[+] 已按标记清理 hosts" -ForegroundColor Green
        }
    }
    ipconfig /flushdns | Out-Null
}

# ---------------- 还原模式 ----------------
if ($Restore) {
    Restore-Hosts
    return
}

# ---------------- 权限检查 ----------------
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] 需要管理员权限（要改 hosts 和监听 80 端口）" -ForegroundColor Red
    Write-Host "    请右键 PowerShell -> 以管理员身份运行，再执行本脚本"
    exit 1
}

# ---------------- 自愈：上次异常退出留下的备份先还原 ----------------
# 放在权限检查之后：非管理员时应当先看到友好的提权提示，而不是一个 Copy-Item 报错。
if (Test-Path -LiteralPath $BackupFile) {
    Write-Host "[*] 检测到上次未清理的 hosts 备份，先还原…" -ForegroundColor Yellow
    try {
        Copy-Item -LiteralPath $BackupFile -Destination $HostsFile -Force -ErrorAction Stop
        Remove-Item -LiteralPath $BackupFile -Force -ErrorAction SilentlyContinue
        ipconfig /flushdns | Out-Null
        Write-Host "[+] 已还原" -ForegroundColor Green
    } catch {
        Write-Host "[!] 还原失败（可能没有写权限）: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host "    请以管理员身份运行本脚本" -ForegroundColor Yellow
        exit 1
    }
}

# ---------------- 解析真实 IP ----------------
Write-Host "`n[*] 解析真实服务器地址…"
$RealIPs = @{}
foreach ($h in $Targets) {
    $ip = $null
    # 先问公共 DNS（避免本地 DNS 被代理劫持，之前实测 198.18.x.x 就是 fake-IP）
    foreach ($dns in @('223.5.5.5', '119.29.29.29', $null)) {
        try {
            if ($dns) {
                $r = Resolve-DnsName -Name $h -Type A -Server $dns -DnsOnly -ErrorAction Stop |
                     Where-Object { $_.IPAddress } | Select-Object -First 1
            } else {
                $r = Resolve-DnsName -Name $h -Type A -ErrorAction Stop |
                     Where-Object { $_.IPAddress } | Select-Object -First 1
            }
            if ($r) { $ip = $r.IPAddress; break }
        } catch { }
    }
    if ($ip) {
        $RealIPs[$h] = $ip
        Write-Host ("    {0,-24} -> {1}" -f $h, $ip) -ForegroundColor DarkGray
    } else {
        Write-Host ("    {0,-24} -> 解析失败，跳过" -f $h) -ForegroundColor Yellow
    }
}
if ($RealIPs.Count -eq 0) {
    Write-Host "[!] 一个都没解析出来，检查网络后重试" -ForegroundColor Red
    exit 1
}

# ---------------- 备份并改写 hosts ----------------
Write-Host "`n[*] 备份并改写 hosts…"
Copy-Item -LiteralPath $HostsFile -Destination $BackupFile -Force
$add = @("", $Marker)
foreach ($h in $RealIPs.Keys) { $add += "127.0.0.1 $h  $Marker" }
Add-Content -LiteralPath $HostsFile -Value $add -Encoding ASCII
ipconfig /flushdns | Out-Null
Write-Host "[+] hosts 已写入（退出时自动还原）" -ForegroundColor Green

# ---------------- 启动监听 ----------------
try {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    $listener.Start()
} catch {
    Write-Host "[!] 无法监听 $Port 端口: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "    可能是 IIS / 其它服务占用。用 -Port 8080 试试（但 hosts 必须走 80，所以优先腾出 80）"
    Restore-Hosts
    exit 1
}

Set-Content -LiteralPath $LogFile -Value "" -Encoding UTF8
Write-Log "=== kwcap 启动，监听 127.0.0.1:$Port ==="
Write-Log "拦截域名: $($Targets -join ', ')"
Write-Host "`n================ 抓包已启动 ================" -ForegroundColor Cyan
Write-Host " 重要：先完全退出酷我（含托盘），再重新打开" -ForegroundColor Yellow
Write-Host " 现在打开酷我，播放一首付费/会员歌曲，保持 10 秒左右" -ForegroundColor Cyan
Write-Host " 然后回到这里按 Ctrl+C 停止" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

# 退出时的兜底还原。
# 注意：引擎事件的 Action 拿不到脚本里定义的函数，所以必须把逻辑内联，
# 变量通过 $global: 传进去。Ctrl+C 不走这里，由下方 finally 负责。
$global:KWCAP_HOSTS  = $HostsFile
$global:KWCAP_BACKUP = $BackupFile
$null = Register-EngineEvent -SourceIdentifier PowerShell.Exiting -Action {
    if ($global:KWCAP_BACKUP -and (Test-Path -LiteralPath $global:KWCAP_BACKUP)) {
        Copy-Item -LiteralPath $global:KWCAP_BACKUP -Destination $global:KWCAP_HOSTS -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $global:KWCAP_BACKUP -Force -ErrorAction SilentlyContinue
    }
}

function Read-HttpRequest {
    param([System.Net.Sockets.NetworkStream]$Stream)

    $buffer = New-Object System.Collections.Generic.List[byte]
    $one = New-Object byte[] 1
    # 逐个字节读到头部结束，避免读多（后面可能是 body）
    while ($true) {
        $n = $Stream.Read($one, 0, 1)
        if ($n -le 0) { break }
        $buffer.Add($one[0])
        $len = $buffer.Count
        if ($len -ge 4 -and
            $buffer[$len-4] -eq 13 -and $buffer[$len-3] -eq 10 -and
            $buffer[$len-2] -eq 13 -and $buffer[$len-1] -eq 10) { break }
        if ($len -gt 65536) { break }
    }
    if ($buffer.Count -eq 0) { return $null }

    $headerText = [System.Text.Encoding]::ASCII.GetString($buffer.ToArray())
    $lines = $headerText -split "`r`n"
    if ($lines.Count -lt 1 -or [string]::IsNullOrWhiteSpace($lines[0])) { return $null }

    $parts = $lines[0] -split ' '
    if ($parts.Count -lt 2) { return $null }
    $method = $parts[0]
    $path = $parts[1]

    $headers = @{}
    for ($i = 1; $i -lt $lines.Count; $i++) {
        $l = $lines[$i]
        if ([string]::IsNullOrWhiteSpace($l)) { continue }
        $idx = $l.IndexOf(':')
        if ($idx -gt 0) {
            $k = $l.Substring(0, $idx).Trim()
            $v = $l.Substring($idx + 1).Trim()
            $headers[$k] = $v
        }
    }

    # 读 body
    # 注意：必须同时支持 Content-Length 和 Transfer-Encoding: chunked。
    # 之前只处理了 Content-Length，导致客户端用 chunked 发的 POST 请求体被整个丢掉，
    # 服务端收到空 body 就返回 404，抓包日志里也就看不到 BODY。
    $body = New-Object System.Collections.Generic.List[byte]
    $te = ''
    if ($headers.ContainsKey('Transfer-Encoding')) { $te = $headers['Transfer-Encoding'] }

    if ($te -match 'chunked') {
        $buf = New-Object byte[] 1
        while ($true) {
            # 读一行：chunk 长度（十六进制，可能带 ;ext）
            $line = ''
            while ($true) {
                $g = $Stream.Read($buf, 0, 1)
                if ($g -le 0) { break }
                if ($buf[0] -eq 10) { break }
                if ($buf[0] -ne 13) { $line += [char]$buf[0] }
            }
            $hex = ($line -split ';')[0].Trim()
            if (-not $hex) { break }
            $size = 0
            if (-not [int]::TryParse($hex, [System.Globalization.NumberStyles]::HexNumber, $null, [ref]$size)) { break }
            if ($size -le 0) {
                # 末尾 chunk，吃掉结尾 CRLF
                $null = $Stream.Read($buf, 0, 1); $null = $Stream.Read($buf, 0, 1)
                break
            }
            $cb = New-Object byte[] 8192
            $remain = $size
            while ($remain -gt 0) {
                $want = [Math]::Min($cb.Length, $remain)
                $got = $Stream.Read($cb, 0, $want)
                if ($got -le 0) { break }
                for ($j = 0; $j -lt $got; $j++) { $body.Add($cb[$j]) }
                $remain -= $got
            }
            $null = $Stream.Read($buf, 0, 1); $null = $Stream.Read($buf, 0, 1)   # chunk 结尾 CRLF
        }
    } else {
        $cl = 0
        if ($headers.ContainsKey('Content-Length')) { [void][int]::TryParse($headers['Content-Length'], [ref]$cl) }
        if ($cl -gt 0) {
            $buf = New-Object byte[] 8192
            $remain = $cl
            while ($remain -gt 0) {
                $want = [Math]::Min($buf.Length, $remain)
                $got = $Stream.Read($buf, 0, $want)
                if ($got -le 0) { break }
                for ($j = 0; $j -lt $got; $j++) { $body.Add($buf[$j]) }
                $remain -= $got
            }
        }
    }

    return [pscustomobject]@{
        Method  = $method
        Path    = $path
        Headers = $headers
        Body    = [System.Text.Encoding]::UTF8.GetString($body.ToArray())
    }
}

function Invoke-Upstream {
    <#
      流式反向代理：
        · 上游响应头原样转发
        · 响应体边读边写给客户端（遇到 30MB 的 FLAC 也不会把内存撑爆）
        · 只保留前 $SampleSize 字节用于日志
      返回：@{ Sample = byte[]; Total = int }
    #>
    param(
        [string]$HostName,
        [string]$IP,
        $Req,
        [System.IO.Stream]$ClientStream,
        [int]$UpstreamPort = 80,
        [int]$SampleSize = 8192
    )

    $up = [System.Net.Sockets.TcpClient]::new()
    $up.ReceiveTimeout = 20000
    $up.SendTimeout = 20000
    $up.Connect($IP, $UpstreamPort)
    $upStream = $up.GetStream()

    # 重建请求：去掉 Accept-Encoding（强迫服务端返回明文，便于记录），强制短连接
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append("$($Req.Method) $($Req.Path) HTTP/1.1`r`n")
    $skip = @('Accept-Encoding', 'Proxy-Connection', 'Connection', 'Host', 'Transfer-Encoding', 'Content-Length')
    foreach ($k in $Req.Headers.Keys) {
        if ($skip -contains $k) { continue }
        [void]$sb.Append("$k`: $($Req.Headers[$k])`r`n")
    }
    [void]$sb.Append("Host: $HostName`r`n")
    [void]$sb.Append("Connection: close`r`n")

    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($Req.Body)
    if ($bodyBytes.Length -gt 0) {
        [void]$sb.Append("Content-Length: $($bodyBytes.Length)`r`n")
    }
    [void]$sb.Append("`r`n")

    $headBytes = [System.Text.Encoding]::UTF8.GetBytes($sb.ToString())
    $upStream.Write($headBytes, 0, $headBytes.Length)
    if ($bodyBytes.Length -gt 0) { $upStream.Write($bodyBytes, 0, $bodyBytes.Length) }
    $upStream.Flush()

    $sample = New-Object System.IO.MemoryStream
    $total = 0
    $buf = New-Object byte[] 32768
    try {
        while ($true) {
            $n = $upStream.Read($buf, 0, $buf.Length)
            if ($n -le 0) { break }
            $total += $n
            # 原样转发给客户端
            try { $ClientStream.Write($buf, 0, $n) } catch { break }
            # 只留一小段做日志
            if ($sample.Length -lt $SampleSize) {
                $take = [Math]::Min($n, $SampleSize - [int]$sample.Length)
                $sample.Write($buf, 0, $take)
            }
        }
    } finally {
        try { $ClientStream.Flush() } catch { }
        $upStream.Close()
        $up.Close()
    }

    return [pscustomobject]@{ Sample = $sample.ToArray(); Total = $total }
}

# ---------------- 主循环 ----------------
$SeenPaths = @{}
try {
    while ($true) {
        $client = $listener.AcceptTcpClient()
        $remote = $client.Client.RemoteEndPoint

        try {
            $stream = $client.GetStream()
            $req = Read-HttpRequest -Stream $stream
            if ($null -eq $req) { $client.Close(); continue }

            # 找出这个请求属于哪个被拦截的域名
            $hostName = $null
            if ($req.Headers.ContainsKey('Host')) { $hostName = $req.Headers['Host'] }
            if ($hostName) { $hostName = $hostName -replace ':\d+$', '' }

            if (-not $hostName -or -not $RealIPs.ContainsKey($hostName)) {
                # 不是我们关心的域名，直接拒绝（避免影响别的程序）
                $client.Close()
                continue
            }

            # 同一路径（忽略 query）只完整记录第一次，后续 Range 请求只计数
            $sig = $hostName + ($req.Path -replace '\?.*$', '')
            $isFirst = -not $SeenPaths.ContainsKey($sig)
            if ($isFirst) { $SeenPaths[$sig] = 0 } else { $SeenPaths[$sig]++ }

            if (-not $isFirst -and $SeenPaths[$sig] -gt 0) {
                # 重复请求：静默转发，不写日志
                $r0 = Invoke-Upstream -HostName $hostName -IP $RealIPs[$hostName] `
                                      -Req $req -ClientStream $stream -SampleSize 0
                continue
            }

            Write-Log ">>> $($req.Method) http://$hostName$($req.Path)"
            if ($req.Body) {
                $bt = $req.Body
                $bad = $false
                foreach ($ch in $bt.ToCharArray()) {
                    if ([int]$ch -lt 9 -or ([int]$ch -gt 13 -and [int]$ch -lt 32)) { $bad = $true; break }
                }
                if ($bad) {
                    Write-Log "    BODY(base64): $([Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($bt)))"
                } else {
                    Write-Log "    BODY: $bt"
                }
            }
            foreach ($hk in @('Range', 'Referer')) {
                if ($req.Headers.ContainsKey($hk)) { Write-Log "    $hk`: $($req.Headers[$hk])" }
            }

            # 流式转发：响应体直接写给客户端，只回传前 8KB 样本用于记录
            $result = Invoke-Upstream -HostName $hostName -IP $RealIPs[$hostName] `
                                      -Req $req -ClientStream $stream

            $sample = $result.Sample
            $total  = $result.Total

            if ($total -eq 0) {
                Write-Log "    RESP: (空响应)"
            } else {
                $headEnd = -1
                for ($i = 0; $i -lt $sample.Length - 3; $i++) {
                    if ($sample[$i] -eq 13 -and $sample[$i+1] -eq 10 -and
                        $sample[$i+2] -eq 13 -and $sample[$i+3] -eq 10) { $headEnd = $i; break }
                }
                if ($headEnd -ge 0) {
                    $headText = [System.Text.Encoding]::ASCII.GetString($sample, 0, $headEnd)
                    $status = ($headText -split "`r`n")[0]
                    Write-Log "    $status  (总 $total 字节)"

                    # 打印响应头里的关键信息（Content-Type / Content-Range / Content-Encoding）
                    foreach ($hl in ($headText -split "`r`n")) {
                        if ($hl -match '^(Content-Type|Content-Range|Content-Encoding|Content-Length):') {
                            Write-Log "    $hl"
                        }
                    }

                    $bodyOff = $headEnd + 4
                    $bodyLen = $sample.Length - $bodyOff
                    if ($bodyLen -gt 0) {
                        $body = New-Object byte[] $bodyLen
                        [Array]::Copy($sample, $bodyOff, $body, 0, $bodyLen)

                        # 判断是否是可读文本：出现替换符或大量控制字符就按二进制处理
                        $txt = [System.Text.Encoding]::UTF8.GetString($body)
                        if ($txt.IndexOf([char]0xFFFD) -ge 0) {
                            try { $txt = [System.Text.Encoding]::GetEncoding(936).GetString($body) } catch { }
                        }
                        $ctrl = 0
                        foreach ($ch in $txt.ToCharArray()) {
                            if ([int]$ch -lt 32 -and $ch -ne "`r" -and $ch -ne "`n" -and $ch -ne "`t") { $ctrl++ }
                        }
                        $isBinary = ($txt.IndexOf([char]0xFFFD) -ge 0) -or ($ctrl -gt [Math]::Max(4, $txt.Length / 20))

                        if ($isBinary) {
                            # 二进制：打前 8 字节的十六进制（能立刻看出是不是 zlib/gzip）
                            $hex = ($body[0..([Math]::Min(7, $body.Length-1))] | ForEach-Object { $_.ToString('x2') }) -join ' '
                            Write-Log "    BINARY 头部: $hex"
                            Write-Log "    BINARY base64: $([Convert]::ToBase64String($body))"
                        } else {
                            $tail = if ($txt.Length -gt 2000) { $txt.Substring(0,2000) + ' …(截断)' } else { $txt }
                            Write-Log "    RESP: $tail"
                        }
                    }
                } else {
                    Write-Log "    RESP: (无完整响应头，共 $total 字节)"
                }
            }
            Write-Log "    ----"
        } catch {
            Write-Log "    [error] $($_.Exception.Message)"
        } finally {
            $client.Close()
        }
    }
}
finally {
    try { $listener.Stop() } catch { }

    # 内联还原：不依赖 Restore-Hosts 是否还在作用域里
    try {
        if (Test-Path -LiteralPath $BackupFile) {
            Copy-Item -LiteralPath $BackupFile -Destination $HostsFile -Force
            Remove-Item -LiteralPath $BackupFile -Force
            ipconfig /flushdns | Out-Null
            Write-Host "[+] hosts 已还原" -ForegroundColor Green
        }
    } catch {
        Write-Host "[!] 自动还原失败，请手动执行：" -ForegroundColor Red
        Write-Host "    powershell -ExecutionPolicy Bypass -File `"$ScriptDir\kwcap.ps1`" -Restore" -ForegroundColor Yellow
    }

    Write-Log "=== kwcap 停止 ==="
    Write-Host "`n[+] 日志文件: $LogFile" -ForegroundColor Green
    Write-Host "[+] 把上面这个文件发给我即可`n" -ForegroundColor Green
}
