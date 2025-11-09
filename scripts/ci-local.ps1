# Local CI script for Windows - runs the same checks as GitHub Actions CI

$ErrorActionPreference = "Stop"

# Function to print colored output
function Print-Step {
    param([string]$Message)
    Write-Host "==> " -ForegroundColor Blue -NoNewline
    Write-Host $Message -ForegroundColor Green
}

function Print-Error {
    param([string]$Message)
    Write-Host "ERROR: " -ForegroundColor Red -NoNewline
    Write-Host $Message
}

function Print-Success {
    param([string]$Message)
    Write-Host "SUCCESS: " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Print-Warning {
    param([string]$Message)
    Write-Host "WARNING: " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

# Check if cargo is installed
try {
    $null = Get-Command cargo -ErrorAction Stop
} catch {
    Print-Error "Cargo is not installed. Please install Rust from https://rustup.rs/"
    exit 1
}

# Display Rust version
Print-Step "Rust version information"
rustc --version
cargo --version
Write-Host ""

# Check formatting
Print-Step "Checking code formatting..."
try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) {
        throw "Formatting check failed"
    }
    Print-Success "Code formatting check passed"
} catch {
    Print-Error "Code formatting check failed. Run 'cargo fmt --all' to fix."
    exit 1
}
Write-Host ""

# Run clippy
Print-Step "Running clippy lints..."
try {
    cargo clippy --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) {
        throw "Clippy check failed"
    }
    Print-Success "Clippy check passed"
} catch {
    Print-Error "Clippy check failed. Please fix the warnings above."
    exit 1
}
Write-Host ""

# Run tests
Print-Step "Running tests..."
try {
    cargo test --all-features --verbose
    if ($LASTEXITCODE -ne 0) {
        throw "Tests failed"
    }
    Print-Success "All tests passed"
} catch {
    Print-Error "Tests failed"
    exit 1
}
Write-Host ""

# Build release
Print-Step "Building release binary..."
try {
    cargo build --release --verbose
    if ($LASTEXITCODE -ne 0) {
        throw "Build failed"
    }
    Print-Success "Release build successful"
} catch {
    Print-Error "Release build failed"
    exit 1
}
Write-Host ""

Print-Success "All CI checks passed!"
Write-Host ""
Write-Host "Your code is ready to be committed and pushed."
