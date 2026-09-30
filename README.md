# Paperclip Wallet · Beta

A self-hosted Bitcoin Blake2b (XBT) wallet based on Bark by Second and the Bark
contributors. This edition presets new wallets to **https://ark.paperclippool.xyz**.
Existing wallet configuration is preserved. Wallet keys stay on your device.

**Beta. The production Paperclip Ark server is not open for deposits yet.**
See [service status](https://ark.paperclippool.xyz/) before funding.

## Features

- On-chain XBT receipt and payment.
- Ark deposits, transfers, refresh, withdrawal, and emergency exits.
- BOLT11 and BOLT12 Lightning payments when the connected server enables them.
- An authenticated web interface and command-line tools.
- Umbrel community-store and StartOS 0.3.5 package generation.

The wallet needs a compatible XBT blockchain backend. Use an indexed XBT Knots
node, or the private [pruned-node adapter](deployment/PRUNED-NODES.md). SHA-256 BTC nodes
are not compatible. No private RPC credentials or wallet keys are included.

## Install

See [wallet apps](https://ark.paperclippool.xyz/wallet/) and the
[connection guide](https://ark.paperclippool.xyz/connect/). Packages use immutable
container digests. A source wrapper is not an install-tested binary release.
StartOS 0.4 requires a separate wrapper; do not install the 0.3.5 package on 0.4.

See [platform instructions](deployment/PLATFORMS.md) for authentication,
backend setup, packaging, and restore requirements.

## Build

```sh
nix develop --command bash scripts/build.sh
./target/debug/paperclip-wallet --help
./target/debug/paperclip-walletd --help
```

The Docker build uses `deployment/Dockerfile`. Release builds run natively for
amd64 and arm64. The default server applies only to new wallet setup. The daemon
retains explicit network selection and mainnet opt-in outside platform packages.

## Recovery

Back up the complete wallet directory, including signed recovery data. A seed
alone is not a complete Ark recovery backup. Refresh or exit before expiry.
Emergency exits need blockchain access, transaction fees, confirmations, and
timelocks. Do not run two wallets from the same backup.

The daemon holds keys on its host. Never expose its private API or access token
to the internet. The Umbrel app password authenticates access; it does not
generate wallet keys. No shared seed or platform seed is used.

## Attribution

See [UPSTREAM.md](UPSTREAM.md) and the original MIT [LICENSE](LICENSE).
Internal Bark crate and RPC names are retained for source compatibility.
See [FUNDED-EXITS.md](FUNDED-EXITS.md) for recovery reserves and limitations.
