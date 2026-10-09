@echo off
setlocal

echo [INFO] Initializing MSVC 2022 Developer Environment...
if exist "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" (
    call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
) else if exist "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" (
    call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
) else (
    echo [ERROR] Could not find Visual Studio 2022 vcvars64.bat.
    exit /b 1
)

echo [INFO] Setting up Qt 6.8 MSVC Environment...
if exist "D:\Qt\6.8.3\msvc2022_64\bin\qtenv2.bat" (
    call "D:\Qt\6.8.3\msvc2022_64\bin\qtenv2.bat" >nul
) else if exist "C:\Qt\6.8.3\msvc2022_64\bin\qtenv2.bat" (
    call "C:\Qt\6.8.3\msvc2022_64\bin\qtenv2.bat" >nul
) else (
    echo [ERROR] Could not find Qt 6.8.3 msvc2022_64 installation.
    exit /b 1
)

set "MODE=release"
if /i "%~1"=="debug" set "MODE=debug"

echo [INFO] Building UltraStar-Creator (%MODE%)...
cd /d "%~dp0src"

if not exist "tmp\%MODE%" mkdir "tmp\%MODE%"
if not exist "..\bin\%MODE%" mkdir "..\bin\%MODE%"

if /i "%MODE%"=="debug" (
    qmake UltraStar-Creator.pro "CONFIG+=debug"
) else (
    qmake UltraStar-Creator.pro
)

if errorlevel 1 (
    echo [ERROR] qmake configuration failed.
    exit /b 1
)

set CL=/MP
nmake

if errorlevel 1 (
    echo [ERROR] Compilation failed.
    exit /b 1
)

echo [INFO] Successfully built UltraStar-Creator (%MODE%)!
