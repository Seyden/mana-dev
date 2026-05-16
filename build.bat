@echo off
REM Change to the directory where this batch file is located
cd /d "%~dp0"

echo Building Mana Runtime Compare Tool...
echo Current directory: %CD%
echo.

REM Check if Cargo.toml exists in current directory
if not exist "Cargo.toml" (
    echo ERROR: Cargo.toml not found in current directory.
    echo Make sure you're running this script from the project root directory.
    pause
    exit /b 1
)

REM Check if cargo is available
where cargo >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo ERROR: Cargo not found. Please install Rust from https://rustup.rs/
    echo.
    echo After installation, restart your terminal and run this script again.
    pause
    exit /b 1
)

echo Checking Rust installation...
cargo --version

echo.
echo Building project...
cargo build --release

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ✓ Build successful!
    echo.
    echo The executable is available at: target\release\mana-runtime-compare.exe
    echo.
    echo Example usage:
    echo target\release\mana-runtime-compare.exe --file "E:\Wow\Mana-sources\mana\sources\AsuraScans.mana"
) else (
    echo.
    echo ✗ Build failed. Please check the error messages above.
)

pause
