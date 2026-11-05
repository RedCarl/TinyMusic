<#
    kwres.ps1 —— 酷我 VIP 曲目取链器（算法已验证）

    原理
    ----
    1. search.kuwo.cn  stype=musicinfo  → 拿到每种音质的 S1/S2/SIZE
    2. deliver.kuwo.cn /yl_res_manage.search（POST 密文）→ 返回带签名直链
       请求/响应都是「两轮 旋转+定长异或」加密，密钥来自 lidx.dll：
         K13 = KoOtOiTvINGwd
         K23 = _Y8g2E6n0E1i7L5t2IoOoNk
    3. 拿到 http://kw-*.kuwo.cn/{token}/{ts}/resource/{mid}/trackmedia/{file}.flac

    用法
    ----
    # 自检（离线，验证加密实现与真实客户端字节级一致）
    powershell -ExecutionPolicy Bypass -File .\kwres.ps1 -SelfTest

    # 取链（只打印，不下载）
    powershell -ExecutionPolicy Bypass -File .\kwres.ps1 -Rid 440613

    # 取链并下载到当前目录
    powershell -ExecutionPolicy Bypass -File .\kwres.ps1 -Rid 440613 -Download
#>

[CmdletBinding()]
param(
    [string]$Rid,
    [string]$Fmt = 'ALFLAC',
    [switch]$Download,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'

$K13 = [System.Text.Encoding]::ASCII.GetBytes('KoOtOiTvINGwd')
$K23 = [System.Text.Encoding]::ASCII.GetBytes('_Y8g2E6n0E1i7L5t2IoOoNk')
$UA  = 'kwplayer_pc_8.7.4.0'
$Host0 = 'deliver.kuwo.cn'

# ---------------------------------------------------------------- 加密核心

function Rot-Left {
    param([byte[]]$B, [int]$R)
    if ($B.Length -eq 0) { return $B }
    $R = ((($R % $B.Length) + $B.Length) % $B.Length)
    if ($R -eq 0) { return $B }
    $out = New-Object byte[] $B.Length
    [Array]::Copy($B, $R, $out, 0, $B.Length - $R)
    [Array]::Copy($B, 0, $out, $B.Length - $R, $R)
    return $out
}

function Rot-Right {
    param([byte[]]$B, [int]$R)
    if ($B.Length -eq 0) { return $B }
    $R = ((($R % $B.Length) + $B.Length) % $B.Length)
    if ($R -eq 0) { return $B }
    $out = New-Object byte[] $B.Length
    [Array]::Copy($B, $B.Length - $R, $out, 0, $R)
    [Array]::Copy($B, 0, $out, $R, $B.Length - $R)
    return $out
}

function Xor-Key {
    param([byte[]]$B, [byte[]]$K)
    $out = New-Object byte[] $B.Length
    for ($i = 0; $i -lt $B.Length; $i++) {
        $out[$i] = $B[$i] -bxor $K[$i % $K.Length]
    }
    return $out
}

function Encrypt-Msg {
    param([byte[]]$B)
    $x = Rot-Right -B $B -R 133
    $x = Xor-Key -B $x -K $K13
    $x = Rot-Left  -B $x -R 71
    $x = Xor-Key -B $x -K $K23
    return $x
}

function Decrypt-Msg {
    param([byte[]]$B)
    $x = Xor-Key -B $B -K $K23
    $x = Rot-Right -B $x -R 71
    $x = Xor-Key -B $x -K $K13
    $x = Rot-Left  -B $x -R 133
    return $x
}

# ---------------------------------------------------------------- 自检

if ($SelfTest) {
    # 真实抓包向量：客户端发出的明文 与 线上密文
    # 直接用 base64 嵌入真实明文（末尾含 CRLF，共 190 字节）
    $plainB64 = 'PDAwMT48VV9RUlk+fDwxNDY4NDkyODA1LDMxNjY4MzI0NzI+fDw1ODM2NTYyND48TVVTSUNfOS42LjAuMF9XMT48a3dtdXNpY193ZWJfMV9iZHNfMjAxNzEyMjIuZXhlPnw8Pnw8cmlkPnw8dWlwOjAuMC4wLjA+fDxuZXc+fDxuYXQ6Mz58PGZsYWdzOjI2MjE0ND48c3BlZXI+fDxpcGRlbnk6bm8+fDxsb2dpbmlkOjY0NTc5Mzk2Nj4NCg=='
    $plainBytes = [Convert]::FromBase64String($plainB64)
    $plain = [System.Text.Encoding]::ASCII.GetString($plainBytes)
    $cipherB64 = 'ZU4FE0YMLhk5ZiRBLSJxC04/LB8aHVdbczRBenBEQDdFH2gBAhRyWEMabUdeZnpfEylLGFcvQDhPKGU5VzF6NjYqJztwR38OdWN2fhBtJ38UQDl6IEwKEwoWEQcaIkIXdx8BPUoJTTZWDE4OSwJGUVNIXE8JQidXGlcqIHtUIUJpBW4jcWljRHZnaXBAdhdWIXptOnQRbypxD1JrYE4aXRgVam1BLWhGVXtUE3ZCCFQtSA8IMRZFM3pTAC0BEA=='
    $cipher = [Convert]::FromBase64String($cipherB64)

    Write-Host "明文长度: $($plainBytes.Length)  密文长度: $($cipher.Length)"
    if ($plainBytes.Length -ne $cipher.Length) {
        Write-Host "[!] 长度不符，向量无效" -ForegroundColor Red; exit 1
    }

    $got = Encrypt-Msg -B $plainBytes
    $same = $true
    for ($i = 0; $i -lt $cipher.Length; $i++) {
        if ($got[$i] -ne $cipher[$i]) { $same = $false; break }
    }
    if ($same) {
        Write-Host "[OK] 加密结果与真实客户端密文 字节级完全一致" -ForegroundColor Green
    } else {
        Write-Host "[X] 加密不匹配，首个差异位置: $i" -ForegroundColor Red
        exit 1
    }

    $back = Decrypt-Msg -B $cipher
    $backText = [System.Text.Encoding]::ASCII.GetString($back)
    if ($backText -eq $plain) {
        Write-Host "[OK] 解密还原成功" -ForegroundColor Green
        Write-Host "     明文: $backText"
    } else {
        Write-Host "[X] 解密失败: $backText" -ForegroundColor Red
        exit 1
    }
    return
}

# ---------------------------------------------------------------- 业务

function Get-MusicInfo {
    param([string]$MusicRid)
    $u = "http://search.kuwo.cn/r.s?stype=musicinfo&itemset=music_2014&alflac=1&pcmp4=1&ids=MUSIC_$MusicRid"
    # 必须用 WebClient.DownloadData 取原始字节：
    # Invoke-WebRequest 的 .Content 在 PowerShell 5.1 会把二进制当字符串解码，数据直接损坏
    $wc = New-Object System.Net.WebClient
    $wc.Headers.Add('User-Agent', $UA)
    $b = $wc.DownloadData($u)
    $wc.Dispose()
    # 包格式: [u32 解压后长度][u32 压缩长度][zlib 流]
    $payload = New-Object byte[] ($b.Length - 8)
    [Array]::Copy($b, 8, $payload, 0, $payload.Length)
    # zlib = 2 字节头 + raw deflate + 4 字节 adler32
    $body = New-Object byte[] ($payload.Length - 2)
    [Array]::Copy($payload, 2, $body, 0, $body.Length)
    $ms = New-Object System.IO.MemoryStream(,$body)
    $ds = New-Object System.IO.Compression.DeflateStream($ms, [System.IO.Compression.CompressionMode]::Decompress)
    $out = New-Object System.IO.MemoryStream
    $ds.CopyTo($out)
    $ds.Dispose(); $ms.Dispose()
    return [System.Text.Encoding]::UTF8.GetString($out.ToArray())
}

function Get-ResourceUrl {
    param([string]$MusicRid, [string]$QualityFmt)

    $info = Get-MusicInfo -MusicRid $MusicRid
    $pat = "$QualityFmt=S1:(\d+)\|S2:(\d+)\|SIZE:(\d+)"
    $m = [regex]::Match($info, $pat)
    if (-not $m.Success) { throw "该曲目没有 $QualityFmt 音质" }
    $s1 = $m.Groups[1].Value
    $s2 = $m.Groups[2].Value
    $size = [int64]$m.Groups[3].Value
    Write-Host "[*] $QualityFmt  S1=$s1 S2=$s2 SIZE=$size"

    $plain = "<001><U_QRY>|<$s1,$s2>|<58365624><MUSIC_9.6.0.0_W1><kwmusic_web_1_bds_20171222.exe>|<>|<rid>|<uip:0.0.0.0>|<new>|<nat:3>|<flags:262144><speer>|<ipdeny:no>|<loginid:645793966>`r`n"
    $cipher = Encrypt-Msg -B ([System.Text.Encoding]::ASCII.GetBytes($plain))

    # 关键：请求体是「密文的 base64 文本」，不是原始二进制。
    # 抓包日志里那行标记为 BODY:（可打印）而不是 BODY(base64):，就是证据。
    $cipherText = [Convert]::ToBase64String($cipher)
    $bodyBytes = [System.Text.Encoding]::ASCII.GetBytes($cipherText)

    $wc2 = New-Object System.Net.WebClient
    $wc2.Headers.Add('User-Agent', $UA)
    $wc2.Headers.Add('Content-Type', 'application/octet-stream')
    $rb = $wc2.UploadData("http://$Host0/yl_res_manage.search", 'POST', $bodyBytes)
    $wc2.Dispose()

    # 响应的线上格式是 base64 文本（请求是原始二进制，两者不对称）
    $rbText = [System.Text.Encoding]::ASCII.GetString($rb).Trim()
    if ($rbText -match '^<(!DOCTYPE|html)' -or $rbText -match '^[\s\S]*<html>') {
        throw "服务端返回了 HTML 错误页，说明当前网络没打到真实节点：$($rbText.Substring(0,[Math]::Min(120,$rbText.Length)))"
    }
    try {
        $cipherResp = [Convert]::FromBase64String($rbText)
    } catch {
        throw "响应不是合法的 base64（前 60 字符: $($rbText.Substring(0,[Math]::Min(60,$rbText.Length)))）"
    }
    if ($cipherResp.Length -lt 60) {
        throw "服务端返回了 $($cipherResp.Length) 字节的占位包（base64=$rbText）。" +
              "说明当前网络打到的不是真实节点，换台机器或换网络重试。"
    }

    $txt = [System.Text.Encoding]::ASCII.GetString((Decrypt-Msg -B $cipherResp))
    $urls = [regex]::Matches($txt, 'https?://[^\s<>|\x00-\x1f]+') | ForEach-Object { $_.Value }
    return [pscustomobject]@{ Urls = $urls; Size = $size; Raw = $txt }
}

if (-not $Rid) {
    Write-Host "用法: .\kwres.ps1 -Rid 440613 [-Fmt ALFLAC|MP3H|MP3128] [-Download]"
    Write-Host "      .\kwres.ps1 -SelfTest"
    return
}

$r = Get-ResourceUrl -MusicRid $Rid -QualityFmt $Fmt
Write-Host "[*] 响应明文: $($r.Raw.Substring(0, [Math]::Min(300, $r.Raw.Length)))"
if ($r.Urls.Count -eq 0) {
    Write-Host "[X] 响应里没有 URL" -ForegroundColor Red
    exit 1
}
foreach ($u in $r.Urls) { Write-Host "[+] $u" -ForegroundColor Green }

if ($Download) {
    $u = $r.Urls[0]
    $name = "$Rid-$Fmt" + [System.IO.Path]::GetExtension(([Uri]$u).AbsolutePath)
    Write-Host "[*] 下载到 $name ..."
    Invoke-WebRequest -Uri $u -OutFile $name -UserAgent $UA -TimeoutSec 300
    $len = (Get-Item $name).Length
    Write-Host "[+] 完成: $name  $len 字节 (标称 $($r.Size))"
    if ($len -ne $r.Size) { Write-Host "[!] 大小不一致" -ForegroundColor Yellow }
}
