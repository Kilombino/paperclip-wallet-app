# Paperclip Wallet · Beta

A self-hosted Bitcoin Blake2b (XBT) wallet based on Bark by Second and the Bark
contributors. This edition presets new wallets to **https://ark.paperclippool.xyz**.
Existing wallet configuration is preserved. Wallet keys stay on your device.

**Public beta. Paperclip Ark is open for XBT deposits, Ark transfers, and Lightning payments.**
Use wallet **0.7.6 or later**. Check [service status](https://ark.paperclippool.xyz/) before funding.

Current beta limits: boarding starts at **20,000 sats**; Lightning payments are
limited to **250,000 sats**. Fees and recovery reserves apply. Very small Lightning
payments can be below the funded-HTLC minimum. The server can change these limits.

## Security disclosure

**Not independently audited.** Paperclip Wallet and its Ark integration are
experimental and provided without warranty. Functional tests are not a security
audit. Bugs can cause loss of funds. Use only amounts you can afford to lose.

## What changed in 0.7.6

Adds explicit unaudited-code warnings and a setup acknowledgment. Includes the
0.7.5 interface improvements: clearer Lightning send and receive flows, readable activity and VTXO dashboards,
block-based expiry warnings, and guided recovery controls. Optional tab-scoped
sessions survive page refresh; Lock clears the saved token. Visible tabs refresh
balances without resubmitting payments. Reduced-motion settings are respected.

## Features

- On-chain XBT receipt and payment.
- Ark deposits, transfers, refresh, withdrawal, and emergency exits.
- BOLT11 and BOLT12 Lightning payments when the connected server enables them.
- An authenticated web interface and command-line tools.
- Automatic VTXO refresh while the wallet service is online.
- Umbrel community-store and StartOS 0.4 wallet packages.

The wallet needs a compatible XBT blockchain backend. Use an indexed XBT Knots
node, or the private [pruned-node adapter](deployment/PRUNED-NODES.md). SHA-256 BTC nodes
are not compatible. No private RPC credentials or wallet keys are included.

## Install

See [wallet apps](https://ark.paperclippool.xyz/wallet/) and the
[connection guide](https://ark.paperclippool.xyz/connect/). Packages use immutable
container digests. A source wrapper is not an install-tested binary release.
The [StartOS 0.4 wrapper](https://github.com/connorslab/paperclip-wallet-startos) is separate. The legacy 0.3.5 generator is not the current release target.

Current releases:

- [Umbrel community store](https://github.com/connorslab/paperclip-umbrel-app-store): wallet 0.7.6, with a pinned image.
- [StartOS 0.4 installers](https://github.com/connorslab/paperclip-wallet-startos/releases/tag/v0.7.6-beta.1): x86-64 and ARM64 beta packages.

See [platform instructions](deployment/PLATFORMS.md) for authentication,
backend setup, packaging, and restore requirements.

## Build

Umbrel and StartOS are optional. On a compatible Linux build host with Git and
Nix installed, clone this repository and build the CLI and web daemon:

```sh
git clone https://github.com/connorslab/paperclip-wallet-app.git
cd paperclip-wallet-app
nix develop --command bash scripts/build.sh
./target/debug/paperclip-wallet --help
./target/debug/paperclip-walletd --help
```

The Docker build uses `deployment/Dockerfile`. Release builds run natively for
amd64 and arm64. The default server applies only to new wallet setup. The daemon
retains explicit network selection and mainnet opt-in outside platform packages.

## Verification status

Native amd64 and arm64 images pass startup checks. Umbrel 0.7.6 is deployed
and healthy. StartOS 0.4 packages build and pass manifest validation; device
setup and backup/restore verification remain in progress.

Mainnet tests passed for boarding, refresh, Ark transfer, BOLT11 send and
receive, BOLT12 send, cooperative withdrawal, and balance persistence after
restart. A refused receive cancellation preserves its recovery checkpoint.
The launch relies on prior emergency-exit tests; a new 144-block production
emergency exit was not repeated. These checks do not establish compatibility
with every device or node implementation.

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
