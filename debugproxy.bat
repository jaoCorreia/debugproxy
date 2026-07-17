@echo off
setlocal
cd /d "%~dp0"

echo ====================================
echo   Debug Proxy
echo   HTTP proxy with TUI for debugging
echo ====================================
echo.
echo This terminal app works best in Windows Terminal.
echo If the UI looks garbled, run this from Windows Terminal
echo instead of the legacy Command Prompt.
echo.
echo Starting...
echo.

debugproxy.exe

if %ERRORLEVEL% NEQ 0 (
    echo.
    echo The application exited with error code %ERRORLEVEL%.
    echo Check debugproxy-crash.log for details.
)

pause
