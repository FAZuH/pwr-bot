# dev.sh module: plugin view preview (pwr-ext integration, ticket 05).
#
# Builds the pwr-preview tool and the bundled plugin binaries (as cargo
# needs), then runs the tool from the repo root so fixtures land in the
# repo's .scratch/preview/. Arguments after `--` are forwarded to the tool:
#
#   ./dev.sh preview                     # render the `hello` plugin view
#   ./dev.sh preview -- --png            # also render a PNG (needs chromium)
#   ./dev.sh preview -- --plugin settings

cmd_preview() {
    local build_args=(-p pwr-preview -p hello -p settings)
    local run_args=()
    local arg
    for arg in "$@"; do
        if [[ "$arg" == "--png" ]]; then
            # `--png` needs the opt-in cargo feature (see
            # crates/preview/Cargo.toml); detect it here so callers never
            # have to know about the wiring.
            build_args+=(--features pwr-preview/png)
            run_args+=(--features pwr-preview/png)
        fi
    done

    inf "Building preview tool and plugin binaries..."
    (cd "$SCRIPT_DIR" && cargo build "${build_args[@]}")
    scs "Build completed"

    # `cargo run` lets cargo locate the binary itself, so a CARGO_TARGET_DIR
    # override (or a custom target dir) keeps working; without the `png`
    # feature in run_args the run stays light exactly as the crate defines.
    inf "Rendering plugin view preview..."
    (cd "$SCRIPT_DIR" && cargo run -q -p pwr-preview "${run_args[@]}" -- "$@")
}

dev_desc preview "Render a plugin view to .scratch/preview (append -- <args> for --png, --plugin)"
