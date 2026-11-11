#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
for bin in solana anchor yarn; do command -v "$bin" >/dev/null || { echo "Missing $bin"; exit 1; }; done
mkdir -p target/deploy
if [[ ! -f target/deploy/workspace-keypair.json ]]; then
  echo 'Generating a NEW devnet program keypair locally. Do not commit it.'
  solana-keygen new --no-bip39-passphrase --silent --outfile target/deploy/workspace-keypair.json
fi
anchor keys sync
PROGRAM_ID="$(solana-keygen pubkey target/deploy/workspace-keypair.json)"
echo "Program ID: $PROGRAM_ID"
solana config set --url devnet
solana address
solana balance --url devnet
printf '\nBuild using: yarn install && anchor build\n'
printf 'After independent security review, deploy using: anchor deploy --provider.cluster devnet\n'
