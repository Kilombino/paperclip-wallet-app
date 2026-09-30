# Paperclip Wallet on Umbrel and StartOS

These are beta package sources. Image builds and package verification are required before installation.
The source container targets Linux amd64 and arm64. Current StartOS 0.4 packages use https://github.com/connorslab/paperclip-wallet-startos. The generator below retains a legacy 0.3.5 wrapper for reference; it is not the 0.4 package.

## Setup

Automatic VTXO refresh is enabled by default while the wallet service is online.
The daemon joins server rounds for eligible balances before expiry. The default
mainnet refresh threshold is 144 blocks. Keep the server and blockchain backend
reachable, and allow for refresh fees. Closing the browser does not stop refresh.
Umbrel restarts the wallet after host restarts unless you explicitly stop it.
StartOS manages the service lifecycle. Stopping the app stops maintenance.
Spent, withdrawn, or exiting VTXOs are not selected for new refresh rounds.
Existing manually configured wallets keep their saved settings; make sure
`daemon_manual_sync` is false if you want automatic maintenance.

Open the platform's private wallet interface. Umbrel users unlock with their
app password. StartOS owners use **Actions → Show wallet access token**, then
paste the token into the wallet. This token grants spending access.

On a new installation, choose XBT mainnet or regtest, keep the preset https://ark.paperclippool.xyz server URL,
and enter your selected XBT Knots RPC endpoint and credentials. Use the
platform's private service network or a trusted encrypted tunnel. Do not expose
Knots RPC or wallet ports directly to the Internet. No Bitcoin-node app ID,
vendor, or container address is hardcoded. Platform service discovery remains
to be implemented; enter the endpoint explicitly for now.

The backend requires an activated XBT node and synchronized transaction history.
A pruned companion node can use the bundled private chain adapter described in
`BUNDLED-PRUNED.md`. It starts with the wallet after you configure it. Its index
and credentials persist in the app volume. Retropex, privkeyio, and Paul Lamb variants are compatibility targets,
not all tested combinations. Use the full/indexed variant where available.
The network label `main` alone cannot distinguish XBT from BTC.

The wallet generates its own keys. The Umbrel app password authenticates API
access only; the platform seed is never used as a wallet seed. Password mismatch
after a restore stops startup rather than replacing the saved token.

Before funding, stop the service and back up the complete `data` volume,
including its mnemonic, database, configuration, and authentication token.
Restore the complete volume to a stopped service and retain file ownership
(wallet uid/gid 1000). Seed-only recovery is insufficient for all Ark state.
Do not run two instances from the same backup. The setup form deliberately
does not overwrite an existing wallet or import a seed as a substitute for
a full recovery backup. Live platform backup consistency still needs testing.

Lightning controls activate only when the connected server advertises funded Lightning. The production Paperclip server is not active yet.
The wallet uses the ASP's Lightning service; users do not need their own CLN.
Paul Lamb's Lightning Fork is LND-based and needs a separate ASP adapter.

## Build beta packages

From the repository root, build each architecture natively, or use a configured
Buildx builder with native amd64 and arm64 workers:

```sh
docker buildx build --platform linux/amd64,linux/arm64 \
  -f deployment/Dockerfile --tag YOUR_REGISTRY/paperclip-wallet:YOUR_TAG --push .
python3 deployment/package.py umbrel \
  --image YOUR_REGISTRY/paperclip-wallet:YOUR_TAG@sha256:YOUR_INDEX_DIGEST \
  --output /absolute/new/umbrel/paperclip-wallet
python3 deployment/package.py startos \
  --image YOUR_REGISTRY/paperclip-wallet:YOUR_TAG@sha256:YOUR_INDEX_DIGEST \
  --output /absolute/new/startos/paperclip-wallet
```

Use the actual multi-architecture index digest. The generator validates syntax,
not registry availability or architecture coverage. Publish images only after
testing them. The generated Umbrel directory belongs in an app store with ID
`paperclip`; check port 38180 for conflicts before installation. No host socket,
privileged container, raw wallet port, or disabled platform login is required.

For StartOS, run `make` in the generated wrapper using the 0.3.5 SDK. The wrapper
initializes only its dedicated volume as root and drops to uid 1000 before
starting the wallet. Its token action is available only to the StartOS owner.

Release acceptance still requires native image builds, SDK validation, clean
installation on each platform, setup, wrong-chain rejection, restart, upgrade,
encrypted backup/restore, and an isolated end-to-end transaction/recovery test.
Do not list these apps as production-ready before those pass.

Packaging references: [Umbrel](https://github.com/getumbrel/umbrel-apps/blob/master/.claude/skills/umbrel-package-app/SKILL.md),
[StartOS 0.3.5 wrapper](https://github.com/Start9Labs/bitcoin-core-startos/tree/0.3.5-29.x),
[StartOS 0.4 SDK](https://docs.start9.com/packaging/0.4.0.x/quick-start.html).
