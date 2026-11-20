@echo off
rem Double-click to make a new RideShell release (see scripts\release.ps1).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0release.ps1" %*
echo.
pause
