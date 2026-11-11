#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
for bin in solana solana-keygen anchor yarn; do
  command -v "$bin" >/dev/null || { echo "ERROR: $bin is not installed" >&2; exit 1; }
done
ANCHOR_VERSION="$(anchor --version)"
case "$ANCHOR_VERSION" in *0.31.1*) ;; *) echo "ERROR: Expected Anchor 0.31.1; found $ANCHOR_VERSION" >&2; exit 1;; esac
bash scripts/prepare-devnet.sh
yarn install
anchor build
PROGRAM_ID="$(solana-keygen pubkey target/deploy/workspace-keypair.json)"
DECLARED_ID="$(sed -n 's/.*declare_id!("\([^"]*\)").*/\1/p' programs/workspace/src/lib.rs | head -1)"
if [[ "$PROGRAM_ID" != "$DECLARED_ID" ]]; then
  echo "ERROR: deployed keypair public key does not match declare_id!" >&2; exit 1
fi
for file in target/idl/workspace.json target/types/workspace.ts; do
  [[ -s "$file" ]] || { echo "ERROR: missing generated $file" >&2; exit 1; }
done
printf '\nBUILD COMPLETED\nProgram ID: %s\nIDL: target/idl/workspace.json\nTypes: target/types/workspace.ts\nNo deployment has been performed.\n' "$PROGRAM_ID"
