#!/usr/bin/env bash
set -euo pipefail
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev libudev-dev clang cmake protobuf-compiler libclang-dev
corepack enable || true
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.avm/bin:$PATH"
grep -qxF 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.avm/bin:$PATH"' "$HOME/.bashrc" || echo 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.avm/bin:$PATH"' >> "$HOME/.bashrc"
cargo install --git https://github.com/coral-xyz/anchor avm --locked
avm install 0.31.1
avm use 0.31.1
if [[ ! -f "$HOME/.config/solana/id.json" ]]; then
  mkdir -p "$HOME/.config/solana"
  solana-keygen new --no-bip39-passphrase --silent --outfile "$HOME/.config/solana/id.json"
fi
solana config set --url devnet
echo "Tools installed. In a new terminal: cd serumx-staking-protocol && bash scripts/build-devnet.sh"
