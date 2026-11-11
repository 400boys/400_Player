@echo off
setlocal

echo ============================================================================
echo   400_Player v0.1.0 - Compilation autonome Windows (100%% Pur Rust, 0 DLL)
echo ============================================================================
echo.

where cargo >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo [1/3] Cargo / Rust n est pas encore installe sur ce PC.
    echo       Installation automatique de Rust via winget...
    winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    where cargo >nul 2>nul
    if errorlevel 1 (
        echo.
        echo [ERREUR] Veuillez installer Rust depuis https://rustup.rs puis relancer ce script.
        pause
        exit /b 1
    )
)

echo [1/3] Verification de la chaine de compilation Rust...
cargo --version
rustc --version
echo.

taskkill /F /IM "400 Media Player.exe" >nul 2>nul
taskkill /F /IM "400_Media_Player.exe" >nul 2>nul
if exist ".cargo\config.toml" del /F /Q ".cargo\config.toml"
if exist "400player-crash.log" del /F /Q "400player-crash.log"

echo [2/3] Compilation de 400 Media Player.exe en mode Release optimise...
cargo build --release -p lumen400 --bin 400_Media_Player
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERREUR] La compilation a echoue.
    pause
    exit /b 1
)

echo.
echo [3/3] Preparation du dossier portable dist-windows\...
if not exist "dist-windows" mkdir "dist-windows"
copy /Y "target\release\400_Media_Player.exe" "dist-windows\400 Media Player.exe"
if %ERRORLEVEL% neq 0 (
    echo [ERREUR] Impossible de copier l executable dans dist-windows\.
    pause
    exit /b 1
)
copy /Y "target\release\400_Media_Player.exe" "400 Media Player.exe" >nul
copy /Y "README.md" "dist-windows\README.md" >nul
copy /Y "LICENSE-MIT" "dist-windows\LICENSE-MIT" >nul
copy /Y "NOTICE" "dist-windows\NOTICE" >nul

echo.
echo ============================================================================
echo   TERMINE : Executable autonome pret dans dist-windows\400 Media Player.exe
echo   (Aucune DLL externe ni FFmpeg requis)
echo ============================================================================
echo.
echo Lancement de dist-windows\400 Media Player.exe...
set "WGPU_BACKEND=dx12"
set "RUST_BACKTRACE=1"
"dist-windows\400 Media Player.exe"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [DIAGNOSTIC] L application s est terminee avec le code : %ERRORLEVEL%
    if exist "400player-crash.log" (
        echo --- Contenu de 400player-crash.log ---
        type "400player-crash.log"
    )
)
pause
