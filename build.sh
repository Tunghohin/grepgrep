#!/bin/bash
# Build script for grepgrep
# Supports building for Linux and Windows from any platform

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Detect current platform
detect_platform() {
    case "$(uname -s)" in
        Linux*)     echo "linux";;
        Darwin*)    echo "macos";;
        CYGWIN*|MINGW*|MSYS*)    echo "windows";;
        *)          echo "unknown";;
    esac
}

# Build for Linux
build_linux() {
    print_info "Building for Linux..."

    # Check for Linux toolchain
    if ! rustup target list | grep -q "x86_64-unknown-linux-gnu (installed)"; then
        print_info "Installing Linux target..."
        rustup target add x86_64-unknown-linux-gnu
    fi

    # Build
    cargo build --release --target x86_64-unknown-linux-gnu

    # Create output directory
    mkdir -p dist/linux

    # Copy binary
    cp target/x86_64-unknown-linux-gnu/release/grepgrep dist/linux/

    print_success "Linux build complete: dist/linux/grepgrep"
}

# Build for Windows (requires mingw-w64 or cross-compilation setup)
build_windows() {
    print_info "Building for Windows..."

    local current_platform=$(detect_platform)

    if [ "$current_platform" = "windows" ]; then
        # Native Windows build
        cargo build --release

        mkdir -p dist/windows
        cp target/release/grepgrep.exe dist/windows/ 2>/dev/null || \
        cp target/release/grepgrep dist/windows/grepgrep.exe

        print_success "Windows build complete: dist/windows/grepgrep.exe"
    else
        # Cross-compilation from Linux/macOS
        print_info "Cross-compiling for Windows..."

        # Check for Windows target
        if ! rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
            print_info "Installing Windows target..."
            rustup target add x86_64-pc-windows-gnu
        fi

        # Check for mingw-w64
        if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
            print_warning "mingw-w64 not found. Installing..."
            if command -v apt-get &> /dev/null; then
                sudo apt-get install -y mingw-w64
            elif command -v dnf &> /dev/null; then
                sudo dnf install -y mingw64-gcc
            elif command -v pacman &> /dev/null; then
                sudo pacman -S --noconfirm mingw-w64-gcc
            else
                print_error "Please install mingw-w64 manually for cross-compilation"
                exit 1
            fi
        fi

        # Build with Windows target
        cargo build --release --target x86_64-pc-windows-gnu

        # Create output directory
        mkdir -p dist/windows

        # Copy binary
        cp target/x86_64-pc-windows-gnu/release/grepgrep.exe dist/windows/

        print_success "Windows build complete: dist/windows/grepgrep.exe"
    fi
}

# Build for current platform
build_current() {
    local platform=$(detect_platform)
    print_info "Building for current platform: $platform"

    cargo build --release

    mkdir -p dist/$platform

    case $platform in
        linux|macos)
            cp target/release/grepgrep dist/$platform/
            ;;
        windows)
            cp target/release/grepgrep.exe dist/$platform/ 2>/dev/null || \
            cp target/release/grepgrep dist/$platform/grepgrep.exe
            ;;
    esac

    print_success "Build complete: dist/$platform/"
}

# Build for all platforms
build_all() {
    print_info "Building for all platforms..."
    build_linux
    build_windows
    print_success "All builds complete!"
}

# Clean build artifacts
clean() {
    print_info "Cleaning build artifacts..."
    cargo clean
    rm -rf dist/
    print_success "Clean complete"
}

# Show help
show_help() {
    echo "grepgrep Build Script"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  linux     Build for Linux (x86_64)"
    echo "  windows   Build for Windows (x86_64)"
    echo "  current   Build for current platform"
    echo "  all       Build for all platforms"
    echo "  clean     Remove build artifacts"
    echo "  help      Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 linux          # Build for Linux"
    echo "  $0 windows        # Build for Windows"
    echo "  $0 --platform linux  # Alternative syntax"
    echo ""
    echo "Current platform: $(detect_platform)"
}

# Main entry point
main() {
    local command=${1:-"help"}

    # Handle --platform flag
    if [ "$1" = "--platform" ] && [ -n "$2" ]; then
        command="$2"
    fi

    case $command in
        linux)
            build_linux
            ;;
        windows)
            build_windows
            ;;
        current)
            build_current
            ;;
        all)
            build_all
            ;;
        clean)
            clean
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            print_error "Unknown command: $command"
            show_help
            exit 1
            ;;
    esac
}

main "$@"