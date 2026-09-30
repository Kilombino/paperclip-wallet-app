# Paperclip XBT wallet

This is an experimental self-hosted wallet. The keys stay on the computer running
`paperclip-walletd`; its browser UI connects to that local daemon. Do not use a
daemon hosted by someone else as though it were a browser-only wallet.

## Set up

The browser now supports first-run setup on an empty daemon. Platform users
should follow [Umbrel/StartOS instructions](deployment/PLATFORMS.md); those packages
are still private candidates. Choose your own compatible XBT Knots RPC endpoint
and ASP. No particular provider is required. Existing wallets must be restored
from their complete backup, not recreated through the new-wallet form.

Build with `nix develop --command bash scripts/build.sh`. You need a synchronized
XBT Knots backend with `txindex=1`; keep its RPC private. Create a fresh wallet:

```sh
export PAPERCLIP_XBT_MAINNET=1
./target/debug/paperclip-wallet --datadir "$HOME/.paperclip-xbt" create \
  --mainnet --ark https://YOUR-ASP-HOST \
  --bitcoind http://127.0.0.1:9332 --bitcoind-cookie /absolute/path/to/.cookie
./target/debug/paperclip-walletd --datadir "$HOME/.paperclip-xbt" \
  --host 127.0.0.1 --port 38180
```

For regtest omit the environment opt-in and use `--regtest` with a separate data
directory and backend. Keep the environment opt-in when reopening mainnet wallets.
The daemon must never bind to a public interface. If it runs on another computer,
use SSH forwarding for port 38180 rather than exposing its API.

Read the local token using `paperclip-walletd --datadir ... secret show` and enter
it at `http://127.0.0.1:38180`. Do not put the token in a URL or share it. The page
keeps it only in memory. Confirm **XBT MAINNET** or **XBT REGTEST** before funding.

## Use

1. Back up the complete wallet directory privately before funding. A seed alone
   is insufficient: signed recovery transactions must also be preserved.
2. Obtain an on-chain deposit address. Send a small initial amount and wait for
   confirmation. Select an amount to board into Ark, allowing for recovery reserves
   and miner fees. Boarding needs three confirmations before funds become spendable.
3. Share an Ark receive address or send to another wallet on the same ASP. Transfers
   consume explicit recovery reserves. Lightning payments are currently disabled.
4. Refresh before the expiry heights shown in the wallet. A refresh needs the ASP.
   Keep backups current after transactions and refreshes.
5. Use Withdraw to return Ark funds to your own on-chain wallet cooperatively.

If a submission has an uncertain result, inspect history with the CLI before
retrying. The interface does not automatically retry payments.

## On-chain sends and Lightning controls

Use Send on-chain to pay an external XBT address from your on-chain balance.
Miner fees are additional. The returned transaction ID identifies a submission,
not a confirmation. Activity shows on-chain transactions and Ark movements.

The Lightning panel is integration work in progress: this ASP recovery profile
still blocks Lightning. Its controls call the existing wallet APIs for paying
BOLT11 invoices, BOLT12 offers, Lightning addresses and LNURL requests, creating
BOLT11 invoices, and reading payment status. They do not bypass the server gate.
Payment controls remain disabled unless the local wallet explicitly advertises
Lightning support; the current build does not advertise it. Status reads remain
available while payment creation is disabled.
Leave the amount blank only when the request supplies it. Incoming payments will
settle into Ark and outgoing payments will use Ark funds; there is no separate
Lightning balance. Receive fees can reduce the amount credited.

After any uncertain submission, use Check payment and Activity before retrying.
The interface does not label a submitted Lightning payment as settled. Keep the
daemon running to progress pending actions. Full Lightning settlement and HTLC
recovery are not yet validated in the XBT funded profile.

## Recover without the ASP

The local UI can unlock and identify the network without contacting the ASP.
Start Emergency Exit, progress it as blocks arrive, and claim matured exits to
your on-chain wallet. Keep the daemon running and the backend reachable. You must
begin before expiry; recovery is not guaranteed after indefinite offline time.
Fixed reserves do not guarantee timely confirmation at arbitrary fee levels.

The CLI provides the same recovery operations; consult `paperclip-wallet exit
--help`. Preserve the whole wallet directory before upgrades. Existing version-3
positions retain their signatures and original relay limitations; an update cannot
rewrite them. Cooperative refresh creates positions with the new format.
