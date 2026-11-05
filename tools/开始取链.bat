@echo off
title kwres - Kuwo VIP URL resolver
chcp 65001 >nul

if not exist "%~dp0kwres.ps1" (
    echo.
    echo  [!] kwres.ps1 not found in this folder.
    echo      Put kwres.ps1 and this .bat in the SAME folder.
    echo.
    pause
    exit /b 1
)

echo.
echo  ============================================================
echo    Kuwo VIP URL resolver   ^(kwres^)
echo  ============================================================
echo.
echo    Step 1: self-test (offline, verifies the crypto)
echo    Step 2: resolve a song and download it
echo.
echo  ------------------------------------------------------------
echo.

echo  [1/2] Self-test...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0kwres.ps1" -SelfTest
echo.
echo  ------------------------------------------------------------
echo.

set RID=
set /p RID=  Enter song rid [default 440613 = Dao Xiang]: 
if "%RID%"=="" set RID=440613

echo.
echo  [2/2] Resolving rid=%RID% ...
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0kwres.ps1" -Rid %RID% -Download

echo.
echo  ------------------------------------------------------------
echo   Done. Check this folder for the downloaded file.
echo  ------------------------------------------------------------
echo.
pause
