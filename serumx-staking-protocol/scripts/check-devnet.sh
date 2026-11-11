#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
PROGRAM_ID="$(solana-keygen pubkey target/deploy/workspace-keypair.json)"
echo "Checking devnet program $PROGRAM_ID"
solana program show "$PROGRAM_ID" --url devnet
