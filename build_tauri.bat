@echo off
echo ========================================================
echo   VITL Piano - Release & WebView Setup Build Script
echo ========================================================

set RUST_MIN_STACK=67108864
set PATH=D:\pxnxar-build-tools\llvm-mingw\llvm-mingw-20260602-ucrt-x86_64\bin;%PATH%

echo [1/3] Building Main Application Binary...
cd D:\vitl-piano-windows\src-tauri
call cargo build --release
if %ERRORLEVEL% neq 0 (
    echo Error building main app!
    exit /b %ERRORLEVEL%
)

echo [2/3] Building Custom WebView Setup & Updater...
cd D:\vitl-piano-windows\installer
call cargo build --release
if %ERRORLEVEL% neq 0 (
    echo Error building installer!
    exit /b %ERRORLEVEL%
)

echo [3/3] Packaging Distribution Artifacts...
cd D:\vitl-piano-windows
if not exist dist mkdir dist
copy /y src-tauri\target\release\vitl-piano.exe dist\vitl-piano.exe
copy /y installer\target\release\vitl-piano-setup.exe dist\vitl-piano-setup.exe
copy /y src-tauri\WebView2Loader.dll dist\WebView2Loader.dll

echo ========================================================
echo   BUILD SUCCESSFUL!
echo   Outputs available in 'dist\' folder:
echo   - dist\vitl-piano-setup.exe  (Custom WebView Installer/Updater)
echo   - dist\vitl-piano.exe        (Standalone Application Binary)
echo ========================================================
