# Bundled pruned-node adapter

Both wallet packages include the private XBT history adapter. It is optional.
An indexed XBT node can connect directly. A pruned node uses a local index in
the same persistent app volume. No additional host service is required.

The adapter is not enabled merely by installing the app. Configure it before
creating a wallet that uses a pruned backend. The first scan covers XBT history
from block 961640 and can take hours. Full peers must retain the missing blocks.

## Configure

Open a terminal inside the wallet container as its wallet user (uid 1000).
Run `python3 /usr/local/lib/paperclip/managed-chain.py configure`.
Enter `main`, the private XBT RPC URL, and the node credentials. The password
prompt does not echo. The command checks the XBT header format before saving.
The supervisor starts the adapter automatically. It restarts failed adapters
and stops the adapter with the wallet app.

For Umbrel, the container is `paperclip-wallet_wallet_1`. For StartOS, use its
service container terminal. Configuration is stored privately under
`/data/chain`. Backend credentials never appear in process arguments.

Run `python3 /usr/local/lib/paperclip/managed-chain.py status` to see index
progress. Wait for `synced: true`. Then run
`python3 /usr/local/lib/paperclip/managed-chain.py connection` to retrieve the
private local adapter connection for the wallet's setup form. This output
contains a private RPC password. Do not publish it.

The local RPC address is `http://127.0.0.1:18336`. It only listens inside the
wallet container. The wallet recognizes its explicit coverage handshake.
Missing history fails closed; it is never treated as an empty balance.

Existing saved wallets keep their backend configuration. Do not change a live
wallet's backend without a complete backup. If a saved wallet uses the adapter,
startup waits for synchronized history. The app may report unhealthy while
this initial or catch-up scan runs. The adapter status command remains usable.

## Persistence and recovery

The app volume includes wallet data and the index. Stop the app before making
a manual filesystem backup, or use SQLite's online backup mechanism. Do not
copy only the main SQLite file while it is running. Preserve the WAL files.
Index data can be rebuilt from peers; wallet recovery state cannot be replaced
by the index. Back up the complete wallet separately from its seed.

Pre-XBT-activation wallet history requires earlier coverage or a full indexed
backend. No archive availability or recovery from indefinite downtime is promised.
