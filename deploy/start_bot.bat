@echo off
rem Start the Lichess bot with a choice of bullet / blitz / rapid (see start_bot.py).
rem Usage: double-click, or: start_bot.bat [bullet|blitz|rapid|mixed|none]
chcp 65001 >nul
cd /d "%~dp0lichess-bot"
if not exist "venv\Scripts\python.exe" (
  echo lichess-bot venv not found: see deploy\README.md
  pause
  exit /b 1
)
"venv\Scripts\python.exe" "..\start_bot.py" %*
pause
