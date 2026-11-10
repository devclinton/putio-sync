@echo off
rem Run by the "putio-sync" scheduled task as SYSTEM at startup (feature 053).
rem With no config yet the daemon starts in setup mode; the setup code is in the log.
set RUST_LOG=info
if not exist "%ProgramData%\putio-sync" mkdir "%ProgramData%\putio-sync"
"%~dp0putio-sync.exe" --config "%ProgramData%\putio-sync\config.toml" daemon >> "%ProgramData%\putio-sync\putio-sync.log" 2>&1
