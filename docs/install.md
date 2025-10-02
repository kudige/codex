## Install & build

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

### DotSlash

The GitHub Release also contains a [DotSlash](https://dotslash-cli.com/) file for the Codex CLI named `codex`. Using a DotSlash file makes it possible to make a lightweight commit to source control to ensure all contributors use the same version of an executable, regardless of what platform they use for development.

### Build from source

```bash
# Clone the repository and navigate to the root of the Cargo workspace.
git clone https://github.com/openai/codex.git
cd codex/codex-rs

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy

# Build Codex.
cargo build

# Launch the TUI with a sample prompt.
cargo run --bin codex -- "explain this codebase to me"

# After making changes, ensure the code is clean.
cargo fmt -- --config imports_granularity=Item
cargo clippy --tests

# Run the tests.
cargo test
``` 
### Install the CLI binaries

Both the interactive TUI (`codex`) and the non-interactive runner (`codex-exec`) can be installed with `cargo install`. By default, Cargo places binaries in `~/.cargo/bin`. Use `--root` when you want them installed in a different prefix.

```bash
# From the repository root
cargo install --path codex-rs/cli --locked --root /path/to/install

# Install the non-interactive binary as well (optional)
cargo install --path codex-rs/exec --bin codex-exec --locked --root /path/to/install
```

Add `/path/to/install/bin` to your `PATH`, or call the binaries with their absolute paths. For example:

```bash
export PATH="/path/to/install/bin:$PATH"

# Interactive mode
codex

# Non-interactive mode with session persistence / transcript logging
codex exec --transcript-log ~/codex.log "summarise README"

# Keep auto-resume metadata next to your project
codex exec --session-store /path/to/project "continue"
```
