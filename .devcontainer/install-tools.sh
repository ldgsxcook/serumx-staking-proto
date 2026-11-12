#!/usr/bin/env bash
set -euo pipefail
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev libudev-dev clang cmake protobuf-compiler libclang-dev
corepack enable || true
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
echo 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"' >> "$HOME/.bashrc"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cargo install --git https://github.com/coral-xyz/anchor avm --locked || true
if command -v avm >/dev/null 2>&1; then avm install 0.31.1 && avm use 0.31.1; fi
printf '\nRun: cd serumx-staking-protocol && bash scripts/build-devnet.sh\n'
