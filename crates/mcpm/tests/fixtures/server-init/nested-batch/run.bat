@echo off
setlocal
cd /d "%~dp0"
set "JVM=-Xmx2G"
call scripts\start.bat
endlocal
