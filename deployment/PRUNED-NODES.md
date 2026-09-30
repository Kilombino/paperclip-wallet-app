# Pruned XBT backend

The private `pruned_rpc.py` adapter adds transaction lookup to a pruned Knots
node. It does not change node consensus, wallet keys, fees, or transaction
construction. It does not expose wallet RPC methods.

The index stores transaction IDs and block locations, not the full blockchain.
It caches raw transactions that applications request. Knots fetches missing
blocks from full peers through `getblockfrompeer`. Knots remains the chain
authority. Historical data availability still depends on peers that retain it.

## Coverage

For mainnet, index from height **961640** or earlier. This covers activated XBT
history. The origin is saved with the genesis hash; an existing database cannot
be reused with a different origin or network. Initial indexing must complete
before the ASP or wallet starts. A node that cannot fetch an old block remains
unready. Do not interpret this as an empty wallet.

Unknown transactions return an explicit coverage error. For local recovery
transactions, the wallet supplies raw ancestry without broadcasting it. The
adapter permits a negative lookup only when a known active ancestor proves
that the transaction cannot predate the indexed range. A reorganization
invalidates that proof when its anchor leaves the active chain.

This adapter is not a global Bitcoin transaction index. Pre-activation wallet
restoration requires an index with earlier coverage or a normal full txindex
backend. The default full-node backend remains supported.

## Private deployment

Use a local authenticated RPC connection or an SSH tunnel to your Knots node.
Give the adapter its own client cookie. A cookie file contains `username:secret`
on one line and must be readable only by its service account. The backend cookie
and client cookie are separate. Never expose this adapter publicly.

```sh
python3 deployment/pruned_rpc.py \
  --node-url http://127.0.0.1:18334 \
  --node-cookie /etc/paperclip-chain/backend.cookie \
  --client-cookie /etc/paperclip-chain/client.cookie \
  --database /var/lib/paperclip-chain/index.sqlite \
  --first-height 961640
```

The default listener is `127.0.0.1:18336`. Point the ASP, watchman, or wallet
bitcoind configuration at that address with the client cookie. Set
`PAPERCLIP_PRUNED_RPC=1` on those processes to enable the explicit adapter
handshake. Keep `PAPERCLIP_XBT_MAINNET=1` for mainnet. The adapter reports its
coverage through `getpaperclipindexinfo`; it never fabricates `getindexinfo`.

The backend must allow `getblockfrompeer` and `getpeerinfo`, in addition to
normal block, transaction, mempool, fee, and broadcast RPCs. A restricted RPC
tunnel may need its allowlist extended. No node restart or pruning change is
required for the adapter itself.

Back up the index with SQLite's online backup API or stop the adapter before
copying all of its files. Copying only the main SQLite file while it runs can
lose WAL state. Keep wallet and ASP recovery backups separately.

## Tests

Run inside `nix develop`, with `XBT_BITCOIND` set to the verified XBT Knots
regtest binary:

```sh
just int-pruned
```

With the same built-binary environment used by `scripts/test-pair.sh`, run
`just int-pruned-lifecycle` for the ASP/wallet deposit, transfer, BOLT11/BOLT12
send, incoming Lightning, and offline exit test. Supply the isolated CLN and
hold-plugin paths as `PAPERCLIP_CLN_EXEC` and `PAPERCLIP_CLN_PLUGIN_DIR`.
It routes both applications through the adapter with native txindex
disabled. All node processes remain isolated regtest fixtures.

The real-node fixture creates two isolated regtest nodes. It deletes historical
block files through pruning, retrieves a transaction from a full peer, restarts
the index, replaces a chain tip, and removes peer access. No mainnet wallet is
used. Evidence remains under `.state/pruned-tests`.
