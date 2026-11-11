# SerumX Staking Protocol — reconstructed source scaffold

This project is reconstructed from the user's supplied Rust source and Anchor.toml snippet. It is **not** a verified build or audited program. No devnet deployment or transaction has been performed.

## Requirements
Install compatible Rust, Solana CLI, Anchor CLI 0.31.1, Node.js and Yarn. Check versions with `anchor --version`, `solana --version`, and `rustc --version`.

## Build
1. `yarn install`
2. `anchor build`
3. `anchor keys list` and compare the generated program ID to the configured ID.
4. If the original deployment keypair is unavailable, **do not deploy with the old ID**. Generate a new devnet program keypair and run `anchor keys sync`, then verify `declare_id!` and `[programs.devnet]` match.
5. Only after reviewing and fixing security issues, run `anchor deploy --provider.cluster devnet` with a devnet-funded signer.

`anchor build` should generate `target/idl/workspace.json` and `target/types/workspace.ts`. These are **not included** because the build has not been run.

## Critical review before deployment
- Validate epoch.config == config.key() in ClaimRewards; PDA checks alone do not bind the epoch data to the expected config.
- Revisit cumulative reward accounting and root publication to ensure total per-holder allocations cannot exceed published entitlements, and indexer reports are independently auditable.
- Test and verify `Config::LEN`, `RewardEpoch::LEN`, and `HolderRecord::LEN` against actual serialized sizes.
- Confirm token mint and token-program compatibility, and never use mainnet treasury tokens during devnet tests.
- A balance indexer, Merkle tree generator, deployment scripts, real integration tests, and frontend claim integration remain to be implemented.

The provided Rust source is copied as supplied; it has not been independently compiled here.

## Local devnet preparation
Run `bash scripts/prepare-devnet.sh` on your own machine after installing the CLI prerequisites. It generates the **program** signing keypair in `target/deploy/workspace-keypair.json` and runs `anchor keys sync`. The program ID changes if you don't possess the original program keypair. Keep the keypair private and back it up securely; never upload it to GitHub or share it in chat.

After a successful build and security review, deploy with `anchor deploy --provider.cluster devnet`. Verify using `bash scripts/check-devnet.sh`. Do not initialize the protocol using the real mainnet SerumX mint on devnet; first create a devnet-only test mint. Do not enable claim UI until verified epochs, Merkle proofs, and claim integration are working.

## Reproducible devnet build
After installing the requirements, run `bash scripts/build-devnet.sh` from the project root. It prepares a local program keypair, synchronizes program IDs, installs JS dependencies, builds the Anchor program, and checks that the IDL and TypeScript types were generated. It **does not deploy**. Preserve `target/deploy/workspace-keypair.json` privately and never share it.
