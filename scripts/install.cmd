@echo off
:: ============================================
:: BioShock Remastered Head Tracking - Install
:: ============================================
:: Based on cameraunlock-core/scripts/templates/install.cmd.
:: Only the CONFIG BLOCK below is customised for this mod.
:: ============================================

:: --- CONFIG BLOCK ---
set "MOD_DISPLAY_NAME=BioShock Remastered Head Tracking"
set "GAME_EXE=BioshockHD.exe"
set "GAME_DISPLAY_NAME=BioShock Remastered"
set "STEAM_FOLDER_NAME=BioShock Remastered"
set "STEAM_SUBFOLDER=Build\Final"
set "ENV_VAR_NAME=BIOSHOCK_PATH"
set "MOD_DLL=xinput1_3.dll"
set "MOD_INTERNAL_NAME=BioshockRemasteredHeadTracking"
set "MOD_VERSION=0.1.0"
set "STATE_FILE=.headtracking-state.json"
set "MOD_CONTROLS=Controls (nav cluster / chord):&echo   Home     / Ctrl+Shift+R  Recenter&echo   End      / Ctrl+Shift+H  Toggle tracking&echo   PageUp   / Ctrl+Shift+P  Toggle 6DOF position&echo   Insert   / Ctrl+Shift+X  Toggle reticle&echo   PageDown / Ctrl+Shift+Y  Toggle yaw mode"
:: --- END CONFIG BLOCK ---

call :main %*
set "_EC=%errorlevel%"
echo.
pause
exit /b %_EC%

:main
setlocal enabledelayedexpansion

echo.
echo === %MOD_DISPLAY_NAME% - Install ===
echo.

set "SCRIPT_DIR=%~dp0"
set "GAME_PATH="

:: --- Find game path (resolves to the Build\Final subfolder) ---

:: Command-line argument: accept either game root OR the Build\Final path
if not "%~1"=="" (
    if exist "%~1\%STEAM_SUBFOLDER%\%GAME_EXE%" (
        set "GAME_PATH=%~1\%STEAM_SUBFOLDER%"
        goto :found_game
    )
    if exist "%~1\%GAME_EXE%" (
        set "GAME_PATH=%~1"
        goto :found_game
    )
    echo ERROR: %GAME_EXE% not found under: %~1
    echo.
    exit /b 1
)

:: Environment variable
if defined %ENV_VAR_NAME% (
    call set "_ENV_PATH=%%%ENV_VAR_NAME%%%"
    if exist "!_ENV_PATH!\%STEAM_SUBFOLDER%\%GAME_EXE%" (
        set "GAME_PATH=!_ENV_PATH!\%STEAM_SUBFOLDER%"
        goto :found_game
    )
    if exist "!_ENV_PATH!\%GAME_EXE%" (
        set "GAME_PATH=!_ENV_PATH!"
        goto :found_game
    )
)

call :find_steam_game
if defined GAME_PATH goto :found_game

echo ERROR: Could not find %GAME_DISPLAY_NAME% installation.
echo.
echo Please either:
echo   1. Set %ENV_VAR_NAME% to your game folder (the one containing Build\Final\%GAME_EXE%)
echo   2. Run: install.cmd "C:\path\to\game"
echo.
exit /b 1

:found_game
echo Game found: %GAME_PATH%
echo.

:: --- Check if game is running ---
tasklist /fi "imagename eq %GAME_EXE%" 2>nul | findstr /i "%GAME_EXE%" >nul 2>&1
if not errorlevel 1 (
    echo ERROR: %GAME_DISPLAY_NAME% is currently running.
    echo Please close the game before installing.
    echo.
    exit /b 1
)

:: --- Deploy mod DLL ---
echo Deploying %MOD_DLL%...

set "SRC_DLL=%SCRIPT_DIR%plugins\%MOD_DLL%"
if not exist "%SRC_DLL%" (
    echo ERROR: %MOD_DLL% not found at: %SRC_DLL%
    echo.
    exit /b 1
)

set "DEST_DLL=%GAME_PATH%\%MOD_DLL%"
set "BACKUP_DLL=%DEST_DLL%.backup"

:: Back up original DLL only on first install
if exist "%DEST_DLL%" if not exist "%BACKUP_DLL%" (
    copy /y "%DEST_DLL%" "%BACKUP_DLL%" >nul
    echo   Backed up original to %MOD_DLL%.backup
)

copy /y "%SRC_DLL%" "%DEST_DLL%" >nul
if errorlevel 1 (
    echo ERROR: Failed to copy %MOD_DLL% to %GAME_PATH%
    exit /b 1
)
echo   Installed %MOD_DLL%

:: --- Write state file ---
> "%GAME_PATH%\%STATE_FILE%" (
    echo {
    echo   "mod": {
    echo     "name": "%MOD_INTERNAL_NAME%",
    echo     "version": "%MOD_VERSION%"
    echo   },
    echo   "backup_present": %ERRORLEVEL%
    echo }
)

echo.
echo ========================================
echo   Installation Complete!
echo ========================================
echo.
echo Launch the game normally via Steam.
echo.
if defined MOD_CONTROLS echo %MOD_CONTROLS%
echo.
exit /b 0

:: ============================================
:: Find game in Steam libraries (resolves to Build\Final)
:: ============================================
:find_steam_game
set "STEAM_PATH="

for /f "tokens=2*" %%a in ('reg query "HKLM\SOFTWARE\WOW6432Node\Valve\Steam" /v InstallPath 2^>nul') do set "STEAM_PATH=%%b"
if not defined STEAM_PATH (
    for /f "tokens=2*" %%a in ('reg query "HKLM\SOFTWARE\Valve\Steam" /v InstallPath 2^>nul') do set "STEAM_PATH=%%b"
)

if defined STEAM_PATH (
    if exist "%STEAM_PATH%\steamapps\common\%STEAM_FOLDER_NAME%\%STEAM_SUBFOLDER%\%GAME_EXE%" (
        set "GAME_PATH=%STEAM_PATH%\steamapps\common\%STEAM_FOLDER_NAME%\%STEAM_SUBFOLDER%"
        exit /b 0
    )

    set "VDF_FILE=%STEAM_PATH%\steamapps\libraryfolders.vdf"
    if exist "!VDF_FILE!" (
        for /f "tokens=1,2 delims=	 " %%a in ('findstr /c:"\"path\"" "!VDF_FILE!" 2^>nul') do (
            set "_LIB_PATH=%%~b"
            set "_LIB_PATH=!_LIB_PATH:\\=\!"
            if exist "!_LIB_PATH!\steamapps\common\%STEAM_FOLDER_NAME%\%STEAM_SUBFOLDER%\%GAME_EXE%" (
                set "GAME_PATH=!_LIB_PATH!\steamapps\common\%STEAM_FOLDER_NAME%\%STEAM_SUBFOLDER%"
                exit /b 0
            )
        )
    )
)

exit /b 1
