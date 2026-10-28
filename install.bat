@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Build/install for the current user; optional first argument: install directory.
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
exit /b %errorlevel%
