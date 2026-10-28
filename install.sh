#!/usr/bin/env bash
# Build the GUI and expose it as disk-cleanup in the user's bin directory.
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
bin_dir="${1:-${HOME}/.local/bin}"
mkdir -p -- "$bin_dir"
bin_dir="$(cd -- "$bin_dir" && pwd -P)"
command_path="$bin_dir/disk-cleanup"
binary_path="$project_dir/target/release/diskclean-gui"

if [[ -e "$command_path" && ! -L "$command_path" ]]; then
    printf 'Refusing to replace an existing file: %s\n' "$command_path" >&2
    exit 1
fi

cargo build --manifest-path "$project_dir/Cargo.toml" --locked --release \
    --package diskclean-gui --target-dir "$project_dir/target"
ln -sfnT -- "$binary_path" "$command_path"
printf 'Installed: %s -> %s\nRun: disk-cleanup [directory]\n' "$command_path" "$binary_path"
case ":${PATH}:" in
    *":${bin_dir}:"*) ;;
    *) printf 'Add this directory to PATH in your shell configuration:\nexport PATH=%q:"$PATH"\n' "$bin_dir" ;;
esac
