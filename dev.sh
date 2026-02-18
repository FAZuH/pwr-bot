#!/bin/bash

# Development helper script for pwr-bot
# Usage: ./dev.sh [command1] [command2] ...
#   commands: format | lint | test | build | docs | all | help
#   Multiple commands can be specified and will execute left to right

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

# Help function
show_help() {
    cat << EOF
pwr-bot Development Helper

Usage: ./dev.sh [command1] [command2] ...

Commands:
  format    - Format code with "cargo +nightly fmt --all"
  lint      - Run linter with "cargo clippy --all-targets --all-features --fix --allow-dirty"
  test      - Run tests with "cargo test --all-features"
  build     - Build Docker image
  docs      - Compile Mermaid diagrams to images
  all       - Run format, lint, test, and build in sequence
  help      - Show this help message

Multiple commands can be specified and will execute sequentially from left to right.

Examples:
  ./dev.sh format                  # Format code
  ./dev.sh lint                    # Run linter
  ./dev.sh test                    # Run tests
  ./dev.sh build                   # Build Docker image
  ./dev.sh docs                    # Compile Mermaid diagrams
  ./dev.sh format lint             # Format then lint
  ./dev.sh all                     # Run format, lint, test, and build

EOF
}

# Command implementations
cmd_format() {
    print_info "Formatting code..."
    cargo +nightly fmt --all
    print_success "Formatting completed"
}

cmd_lint() {
    print_info "Linting code..."
    cargo clippy --all-targets --all-features --no-deps --fix --allow-dirty
    print_success "Linting completed"
}

cmd_test() {
    print_info "Running tests..."
    cargo test --all-features
    print_success "Tests completed"
}

cmd_build() {
    print_info "Building Docker image..."
    docker build -t pwr-bot:latest .
    print_success "Docker image built"
}

cmd_docs() {
    print_info "Compiling Mermaid diagrams..."
    
    # Check if mmdc (Mermaid CLI) is installed
    if ! command -v mmdc &> /dev/null; then
        print_warning "Mermaid CLI not found. Installing..."
        npm install -g @mermaid-js/mermaid-cli
    fi
    
    # Create output directory
    mkdir -p docs/diagrams
    
    # Compile each .mmd file to PNG
    print_info "Processing .mmd diagram files..."
    
    for file in docs/diagrams/*.mmd; do
        if [ -f "$file" ]; then
            filename=$(basename "$file" .mmd)
            print_info "Compiling $filename.mmd..."
            mmdc -i "$file" -o "docs/diagrams/${filename}.png" -b transparent -s 4 --width 3840 --height 2160
        fi
    done
    
    print_success "Mermaid diagrams compiled to docs/diagrams/"
}

cmd_all() {
    print_info "Running all tasks..."
    cmd_format
    cmd_lint
    cmd_test
    cmd_build
    print_success "All tasks completed"
}

# Execute a single command
execute_command() {
    local command="$1"
    
    case "$command" in
        format)
            cmd_format
            ;;
        lint)
            cmd_lint
            ;;
        test)
            cmd_test
            ;;
        build)
            cmd_build
            ;;
        docs)
            cmd_docs
            ;;
        all)
            cmd_all
            ;;
        help)
            show_help
            ;;
        *)
            print_error "Unknown command: $command"
            show_help
            exit 1
            ;;
    esac
}

# Main execution
if [ $# -eq 0 ]; then
    show_help
    exit 0
fi

# Execute each command sequentially
for command in "$@"; do
    execute_command "$command"
done
