@echo off
setlocal

echo ============================================================================
echo   Lumen400 v0.1.0 - Compilation autonome Windows (100%% Pur Rust, 0 DLL)
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

taskkill /F /IM lumen400.exe >nul 2>nul
if exist ".cargo\config.toml" del /F /Q ".cargo\config.toml"
if exist "lumen400-crash.log" del /F /Q "lumen400-crash.log"

echo [2/3] Compilation de lumen400.exe en mode Release optimise...
cargo build --release -p lumen400
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERREUR] La compilation a echoue.
    pause
    exit /b 1
)

echo.
echo [3/3] Preparation du dossier portable dist-windows\...
if not exist "dist-windows" mkdir "dist-windows"
copy /Y "target\release\lumen400.exe" "dist-windows\lumen400.exe"
if %ERRORLEVEL% neq 0 (
    echo [ERREUR] Impossible de copier lumen400.exe dans dist-windows\.
    pause
    exit /b 1
)
copy /Y "README.md" "dist-windows\README.md" >nul
copy /Y "LICENSE-MIT" "dist-windows\LICENSE-MIT" >nul
copy /Y "NOTICE" "dist-windows\NOTICE" >nul

echo.
echo ============================================================================
echo   TERMINE : Executable autonome pret dans dist-windows\lumen400.exe
echo   (Aucune DLL externe ni FFmpeg requis)
echo ============================================================================
echo.
echo Lancement de dist-windows\lumen400.exe...
set "WGPU_BACKEND=dx12"
set "RUST_BACKTRACE=1"
"dist-windows\lumen400.exe"
if %ERRORLEVEL% neq 0 (
    echo.
    echo [DIAGNOSTIC] lumen400.exe s est termine avec le code : %ERRORLEVEL%
    if exist "lumen400-crash.log" (
        echo --- Contenu de lumen400-crash.log ---
        type "lumen400-crash.log"
    )
)
pause
