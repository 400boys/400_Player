@echo off
chcp 65001 >nul 2>&1
setlocal EnableDelayedExpansion
cd /d "%~dp0"

echo ============================================================================
echo   400_media_player v0.1.0 - Compilation autonome Windows (100%% Pur Rust, 0 DLL)
echo ============================================================================
echo.

where cargo >nul 2>&1
if errorlevel 1 (
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
        set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    ) else (
        echo [ERREUR] Cargo / Rust n'est pas installe ou n'est pas dans le PATH.
        echo Installez Rust depuis : https://rustup.rs
        pause
        exit /b 1
    )
)

echo [1/3] Verification de la chaine de compilation Rust...
cargo --version
rustc --version
echo.

echo [2/3] Compilation de 400 Media Player.exe en mode Release optimise...
cargo build --release --bin media_player_400
if errorlevel 1 (
    echo.
    echo [ERREUR] La compilation a echoue.
    pause
    exit /b 1
)

echo.
echo [3/3] Preparation du dossier portable dist-windows\...
if not exist "dist-windows" mkdir "dist-windows"
copy /Y "target\release\media_player_400.exe" "dist-windows\400 Media Player.exe" >nul
if errorlevel 1 (
    echo [ERREUR] Impossible de copier l executable dans dist-windows\.
    pause
    exit /b 1
)

echo.
echo ============================================================================
echo   SUCCES ! Executable autonome genere :
echo   dist-windows\400 Media Player.exe
echo ============================================================================
echo.
echo Lancement de dist-windows\400 Media Player.exe...
start "" "dist-windows\400 Media Player.exe"
endlocal
