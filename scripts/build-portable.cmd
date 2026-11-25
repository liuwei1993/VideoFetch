@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-portable.ps1" %*
