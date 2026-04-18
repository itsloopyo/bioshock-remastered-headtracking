@echo off
:: ============================================
:: BioShock Remastered Head Tracking - Uninstall
:: ============================================

:: --- CONFIG BLOCK ---
set "MOD_DISPLAY_NAME=BioShock Remastered Head Tracking"
set "GAME_EXE=BioshockHD.exe"
set "GAME_DISPLAY_NAME=BioShock Remastered"
set "STEAM_FOLDER_NAME=BioShock Remastered"
set "STEAM_SUBFOLDER=Build\Final"
set "ENV_VAR_NAME=BIOSHOCK_PATH"
set "MOD_DLL=xinput1_3.dll"
set "STATE_FILE=.headtracking-state.json"
:: --- END CONFIG BLOCK ---

call :main %*
set "_EC=%errorlevel%"
echo.
pause
exit /b %_EC%

:main
setlocal enabledelayedexpansion

echo.
echo === %MOD_DISPLAY_NAME% - Uninstall ===
echo.

set "GAME_PATH="

if not "%~1"=="" (
    if exist "%~1\%STEAM_SUBFOLDER%\%GAME_EXE%" (
        set "GAME_PATH=%~1\%STEAM_SUBFOLDER%"
        goto :found_game
    )
    if exist "%~1\%GAME_EXE%" (
        set "GAME_PATH=%~1"
        goto :found_game
    )
)

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

echo %GAME_DISPLAY_NAME% not found - nothing to do.
exit /b 0

:found_game
echo Game found: %GAME_PATH%
echo.

tasklist /fi "imagename eq %GAME_EXE%" 2>nul | findstr /i "%GAME_EXE%" >nul 2>&1
if not errorlevel 1 (
    echo ERROR: %GAME_DISPLAY_NAME% is currently running. Close it first.
    exit /b 1
)

set "DEST_DLL=%GAME_PATH%\%MOD_DLL%"
set "BACKUP_DLL=%DEST_DLL%.backup"
set "STATE=%GAME_PATH%\%STATE_FILE%"

:: Restore backup if present; otherwise just remove the mod DLL
if exist "%BACKUP_DLL%" (
    if exist "%DEST_DLL%" del /q "%DEST_DLL%" >nul 2>&1
    move /y "%BACKUP_DLL%" "%DEST_DLL%" >nul
    echo   Restored original %MOD_DLL% from backup
) else (
    if exist "%DEST_DLL%" (
        del /q "%DEST_DLL%" >nul 2>&1
        echo   Removed %MOD_DLL% (no backup was present)
        echo   If the game misbehaves, run: Steam ^> right-click %GAME_DISPLAY_NAME% ^> Properties ^> Verify integrity
    ) else (
        echo   %MOD_DLL% was not present - nothing to remove
    )
)

if exist "%STATE%" del /q "%STATE%" >nul 2>&1

echo.
echo Uninstall complete. Game is now vanilla.
echo.
exit /b 0

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
