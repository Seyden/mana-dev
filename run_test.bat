@echo off
REM Change to the directory where this batch file is located
cd /d "%~dp0"

echo Testing Mana Runtime Compare Tool with test script...
echo Current directory: %CD%
echo.

REM Check if the executable exists
if not exist "target\release\mana-runtime-compare.exe" (
    echo ERROR: Executable not found. Please run build.bat first.
    pause
    exit /b 1
)

echo Running test with test_script.js...
target\release\mana-runtime-compare.exe --file "test_script.js" --iterations 5

echo.
echo Test completed. Press any key to exit.
pause
