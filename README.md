# Paperclip Wallet · Beta

A self-hosted Bitcoin Blake2b (XBT) wallet based on Bark by Second and the Bark
contributors. This edition presets new wallets to **https://ark.paperclippool.xyz**.
Existing wallet configuration is preserved. Wallet keys stay on your device.

**Public beta. Paperclip Ark is open for XBT deposits, Ark transfers, and Lightning payments.**
The current release is **0.8.2 beta**. Check [service status](https://ark.paperclippool.xyz/) before funding.

Current beta limits: boarding starts at **20,000 sats**; Lightning payments are
limited to **250,000 sats**. Fees and recovery reserves apply. Very small Lightning
payments can be below the funded-HTLC minimum. The server can change these limits.

## Security disclosure

**Not independently audited.** Paperclip Wallet and its Ark integration are
experimental and provided without warranty. Functional tests are not a security
audit. Bugs can cause loss of funds. Use only amounts you can afford to lose.

## What changed in 0.8.2

Ordinary Ark transfers negotiate smaller recovery anchors with compatible servers.
One input allocates **2,660 sats without change** or **3,990 sats with change**,
a 33.5% reduction, with no separate Ark-send service fee. Multiple inputs cost more.
Lightning reserves remain unchanged. Older servers and wallets retain the legacy
budget; existing signed recovery history is preserved. Back up the complete wallet
and do not downgrade while transfers are pending. See [validation evidence and
limitations](docs/small-anchor-validation.md).

## What changed in 0.8.1

- Lightning sends try builder-validated input combinations, preferring a usable single VTXO. Fragmented funds no longer force an invalid HTLC or change output when a suitable input is available.
- Lightning fee estimates use the same selection and include recovery reserves. An unbuildable payment returns an error instead of a hypothetical estimate.
- The web wallet shows the estimated total before confirming a Lightning payment with an entered amount. Estimates can change as wallet funds or server fees change. No automatic payment retries or automatic consolidation are added.

No ASP protocol or wallet database change is required. Recovery minimums remain enforced.

## What changed in 0.8.0

- Reusable BOLT12 offers receive Lightning payments onto Ark through a compatible server. Keep the wallet service online to answer invoice requests. The browser can close.
- Receive QR codes for on-chain, Ark, and Lightning are generated locally.
- [On-chain message signing](docs/onchain-message-signing.md) provides BIP322-simple ownership proofs for wallet-owned Taproot addresses in the web UI, CLI, and authenticated API.

Back up the complete wallet data before an upgrade. Do not downgrade after you
create a reusable offer: old wallet binaries cannot read its checkpoint. A seed
alone does not contain all Ark recovery state. Existing 0.7.7 wallets remain
compatible with the updated Paperclip server.

Paid BOLT12 settlement and failure recovery passed isolated XBT regtest tests.
Live mainnet checks verified two distinct invoices from one offer without payment.
The Umbrel upgrade and message signing passed live checks. Signed StartOS packages
were verified; installation and restore on a physical StartOS device remain unverified.

## What changed in 0.7.7

Adds an accurate Ark-send cost preview and an approved debit limit. Retains
unaudited-code warnings and the setup acknowledgment. Includes the
0.7.5 interface improvements: clearer Lightning send and receive flows, readable activity and VTXO dashboards,
block-based expiry warnings, and guided recovery controls. Optional tab-scoped
sessions survive page refresh; Lock clears the saved token. Visible tabs refresh
balances without resubmitting payments. Reduced-motion settings are respected.

## Ark-send cost preview

Before an Ark transfer, the wallet shows the recipient amount, recovery reserve,
service fee, total balance reduction, and estimated remaining spendable balance.
Recovery reserves are not separately refundable deposits. Small transfers can
have a high reserve relative to the payment amount.

Authenticated clients can use `POST /api/v1/fees/ark/send` with
`{"destination":"<Ark address>","amount_sat":10000}`. This read-only endpoint
uses the same input selection and funded transaction builder as a send. It does
not allocate keys, lock funds, request signatures, or submit a payment.

The response includes `recipient_amount_sat`, `recovery_reserve_sat`,
`service_fee_sat`, `total_debit_sat`, `remaining_spendable_sat`, `input_count`,
and `vtxos_spent`. Insufficient funds, dust and required refreshes return an error;
they do not produce a zero-cost estimate. Wallet state can change after a quote.
Pass the approved `total_debit_sat` as `max_total_sat` to `POST /api/v1/wallet/send`
to reject a higher debit before signing. This optional limit is for Ark sends
only. The web interface always supplies it. No funds are reserved by a quote.

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

- [Umbrel community store](https://github.com/connorslab/paperclip-umbrel-app-store): wallet 0.8.2, with a pinned image.
- [StartOS 0.4 installers](https://github.com/connorslab/paperclip-wallet-startos/releases): x86-64 and ARM64 beta packages.

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

Native amd64 and arm64 images pass startup checks. Umbrel 0.7.7 is deployed
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

## Recovery costs

See [recovery cost accounting and reduction work](docs/recovery-costs.md). Zero
Ark transfer service fees do not mean zero total cost. Recovery allocations are
not separately refundable and are not immediate miner fees.
