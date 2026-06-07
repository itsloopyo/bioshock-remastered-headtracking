@echo off
:: ============================================
:: Bioshock Remastered - Install
:: ============================================
:: Thin wrapper - install body lives in cameraunlock-core/scripts/install-body-shim.cmd.

:: --- CONFIG BLOCK ---
set "GAME_ID=bioshock-remastered"
set "MOD_DISPLAY_NAME=BioShock Remastered Head Tracking"
set "MOD_DLLS=xinput1_3.dll"
set "MOD_INTERNAL_NAME=BioshockRemasteredHeadTracking"
set "MOD_VERSION=0.3.4"
set "STATE_FILE=.headtracking-state.json"
set "FRAMEWORK_TYPE=None"
set "MOD_CONTROLS=Controls (nav cluster / chord):&echo   Home     / Ctrl+Shift+T  Recenter&echo   End      / Ctrl+Shift+Y  Toggle tracking&echo   PageUp   / Ctrl+Shift+G  Toggle 6DOF position&echo   PageDown / Ctrl+Shift+H  Toggle yaw mode"
:: --- END CONFIG BLOCK ---

set "WRAPPER_DIR=%~dp0"
set "_BODY=%WRAPPER_DIR%shared\install-body-shim.cmd"
if not exist "%_BODY%" set "_BODY=%WRAPPER_DIR%..\cameraunlock-core\scripts\install-body-shim.cmd"
if not exist "%_BODY%" (
    echo ERROR: install-body-shim.cmd not found in shared\ or ..\cameraunlock-core\scripts\.
    echo If this is a release ZIP, re-download it from GitHub ^(corrupt installer^).
    echo If this is the dev tree, run: git submodule update --init --recursive
    exit /b 1
)
call "%_BODY%" %*
exit /b %errorlevel%
