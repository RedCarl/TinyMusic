@echo off
title kwcap - Kuwo URL capture
chcp 65001 >nul

rem ---- require administrator ----
rem Do NOT use "net session" to test for admin: it fails when the Server
rem service is stopped, which causes an infinite UAC loop.
rem fsutil does not depend on any service, so it is reliable here.
fsutil dirty query %systemdrive% >nul 2>&1
if %errorLevel% neq 0 (
    if /i "%~1"=="elevated" (
        echo.
        echo  [!] Elevation failed.
        echo      Please right-click this file and choose "Run as administrator".
        echo.
        pause
        exit /b 1
    )
    echo [*] Requesting administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList 'elevated' -Verb RunAs"
    exit /b
)

if not exist "%~dp0kwcap.ps1" (
    echo.
    echo  [!] kwcap.ps1 not found in this folder.
    echo      Put kwcap.ps1 and this .bat in the SAME folder.
    echo.
    pause
    exit /b 1
)

echo.
echo  ============================================================
echo    Kuwo URL capture   ^(kwcap^)
echo  ============================================================
echo.
echo   1. Wait until you see "capture started" below
echo   2. Open the Kuwo client
echo   3. Play a VIP/paid song you have NEVER played before
echo      ^(cached songs will NOT trigger a new request^)
echo   4. Let it play for about 10 seconds
echo   5. Come back to this window and press Ctrl+C
echo.
echo   Log file will be: %~dp0kwcap.log
echo.
echo  ------------------------------------------------------------
echo   Tip: to force a fresh request, delete this folder:
echo        C:\ProgramData\kuwodata
echo   ------------------------------------------------------------
echo.
pause

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0kwcap.ps1"

echo.
echo  ------------------------------------------------------------
echo   Stopped.
echo   Log file: %~dp0kwcap.log
echo   Send that file back.
echo  ------------------------------------------------------------
echo.
pause
