@echo off
set "WGPU_BACKEND=dx12"
set "RUST_BACKTRACE=1"
start "" "%~dp0dist-windows\400 Media Player.exe" %*
