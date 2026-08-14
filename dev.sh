#!/bin/bash

# Development helper script
# Usage: ./dev.sh [command1] [command2] ...
#   commands: format | lint | test | docs | demo | all | help
#   plus any commands provided by modules (scripts/dev-*.sh, dev/*.sh, dev-*.sh)
#   Multiple commands can be specified and will execute left to right

set -e

# Resolve script directory so module discovery works from any CWD
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
inf() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

scs() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

err() {
    echo -e "${RED}[ERROR]${NC} $1"
}

wrn() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

# --- Module registry ---
# Modules define cmd_<name>() functions (hyphens in the command name map to
# underscores) and register a one-line help description via dev_desc.

declare -A CMD_DESCS
declare -a CMD_ORDER

dev_desc() {
    local cmd="$1"
    local desc="$2"
    if [[ -z "${CMD_DESCS[$cmd]+x}" ]]; then
        CMD_ORDER+=("$cmd")
    fi
    CMD_DESCS[$cmd]="$desc"
}

# --- Core commands ---

cmd_format() {
    inf "Formatting code..."
    cargo +nightly fmt --all
    scs "Formatting completed"
}
dev_desc format "Format code with \"cargo +nightly fmt --all\""

cmd_lint() {
    inf "Linting code..."
    cargo clippy --workspace --all-targets --all-features --no-deps --fix --allow-dirty
    scs "Linting completed"
}
dev_desc lint "Run linter with \"cargo clippy --workspace --all-targets --all-features --fix --allow-dirty\""

cmd_test() {
    inf "Running tests..."
    cargo test --workspace --all-targets --all-features
    scs "Tests completed"
}
dev_desc test "Run tests with \"cargo test --workspace --all-targets --all-features\""

cmd_docs() {
    inf "Compiling Mermaid diagrams..."

    # Check if mmdc (Mermaid CLI) is installed
    if ! command -v mmdc &> /dev/null; then
        wrn "Mermaid CLI not found. Installing..."
        npm install -g @mermaid-js/mermaid-cli
    fi

    # Create output directory
    mkdir -p docs/diagrams

    # Compile each .mmd file to PNG
    inf "Processing .mmd diagram files..."

    for file in docs/diagrams/*.mmd; do
        if [ -f "$file" ]; then
            filename=$(basename "$file" .mmd)
            inf "Compiling $filename.mmd..."
            mmdc -i "$file" -o "docs/diagrams/${filename}.png" -b transparent -s 4 --width 3840 --height 2160
        fi
    done

    scs "Mermaid diagrams compiled to docs/diagrams/"
}
dev_desc docs "Compile Mermaid diagrams to images"

cmd_demo() {
    inf "Building release binary..."
    cargo build --release
    scs "Release build completed"

    inf "Creating wrapper script..."
    local wrapper_dir="/tmp/tomo-demo-bin"
    mkdir -p "$wrapper_dir"
    cat > "$wrapper_dir/tomo" << SCRIPT
#!/bin/bash
exec $PWD/target/release/tomo --config-path /tmp/tomo-demo "\$@"
SCRIPT
    chmod +x "$wrapper_dir/tomo"
    export PATH="$wrapper_dir:$PATH"
    trap "rm -rf $wrapper_dir" EXIT
    scs "Wrapper created at $wrapper_dir/tomo"

    if ! command -v vhs &> /dev/null; then
        wrn "vhs not found. Install it: https://github.com/charmbracelet/vhs"
    fi

    inf "Running demo tape..."
    vhs scripts/demo.tape
    scs "Demo tape completed"
}
dev_desc demo "Build release, alias, and run vhs demo tape"

cmd_all() {
    inf "Running all tasks..."
    cmd_format
    cmd_lint
    cmd_test
    scs "All tasks completed"
}
dev_desc all "Run format, lint, and test in sequence"

# --- Module discovery ---
# Source order is precedence: last-loaded wins. The synced baseline
# (scripts/dev-*.sh) loads first, so project-local modules (dev/*.sh,
# dev-*.sh) can override it.

discover_modules() {
    local pat f
    for pat in "scripts/dev-*.sh" "dev/*.sh" "dev-*.sh"; do
        shopt -s nullglob
        for f in ${SCRIPT_DIR}/${pat}; do
            [ -f "$f" ] || continue
            inf "Loading module: $(basename "$f")"
            source "$f"
        done
        shopt -u nullglob
    done
}

discover_modules

# --- Help function ---

show_help() {
    cat << EOF
Development Helper Script

Usage: ./dev.sh [command1] [command2] ...

Commands:
EOF
    local cmd
    for cmd in "${CMD_ORDER[@]}"; do
        printf '  %-12s - %s\n' "$cmd" "${CMD_DESCS[$cmd]}"
    done
    cat << EOF

Multiple commands can be specified and will execute sequentially from left to right.

Examples:
  ./dev.sh format                  # Format code
  ./dev.sh lint                    # Run linter
  ./dev.sh test                    # Run tests
  ./dev.sh docs                    # Compile Mermaid diagrams
  ./dev.sh demo                    # Build release, alias, and run demo tape
  ./dev.sh format lint             # Format then lint
  ./dev.sh all                     # Run format, lint, and test

EOF
}

# Execute a single command
execute_command() {
    local command="$1"
    local fn="cmd_${command//-/_}"

    case "$command" in
        help)
            show_help
            ;;
        all)
            cmd_all
            ;;
        *)
            if declare -F "$fn" &>/dev/null; then
                "$fn"
            else
                err "Unknown command: $command"
                show_help
                exit 1
            fi
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
