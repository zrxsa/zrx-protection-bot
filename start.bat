@echo off
title Protection Bot
chcp 65001 > nul
cd /d "%~dp0"
echo ==========================================
echo Starting Protection Bot (Rust)...
echo ==========================================

:loop
if exist "zrx-protection-bot.exe" (
    zrx-protection-bot.exe
) else if exist "target\release\zrx-protection-bot.exe" (
    target\release\zrx-protection-bot.exe
) else (
    echo [ERROR] zrx-protection-bot.exe not found! Please run 'cargo build --release' first.
    pause
    exit /b 1
)

echo.
echo [!] Bot process stopped or crashed. Restarting in 5 seconds...
timeout /t 5 /nobreak > nul
goto loop
