# Small-anchor release validation

Verified 2026-10-03. Runtime ASP `e1d3d451df6f7316b69ac2e2a9f50c98831fd9f4`
and wallet `364a20c` passed workspace checks, four focused funded-builder tests
and seven wallet send-action tests.

All four isolated default-policy lifecycle tests passed: independent Knots relay,
offline unilateral exit, mempool restart, reorg, refresh, offboard, late receipt
and full-backup recovery; pre-capability wallet receiving, sending and exiting;
and watchman protection after refresh and single/multiple-input offboards,
including a small-anchor input. Policy remained `mempooltruc=reject`, normal
dust relay and 1 sat/vB minimum relay.

The harness required sequencing corrections: mine funded parent transactions,
finish the post-confirmation refresh forfeit exchange, and await P2P relay
before reading watchman transactions from the independent node. The first three
tests passed together; the corrected watchman test passed all four scenarios
in a focused run. No recovery assertion was removed.

The broader library suite has 36 pre-existing fixture failures, identical on the
pre-change source (121 passes) and candidate (122 passes). It is not fully green.
No mainnet payments were initiated for this release.

ASP image build: https://github.com/connorslab/paperclip-asp/actions/runs/37162097479
Wallet build: https://github.com/connorslab/paperclip-wallet-app/actions/runs/37162099040
Bundled packages: https://github.com/connorslab/paperclip-wallet-app/actions/runs/37162659589

The ASP production upgrade completed with an encrypted pre-upgrade snapshot and
a retained previous container. Admin wallet RPC is healthy; public Ark-info
reports exit profile 2, funded Lightning enabled, and small-anchor transfers
enabled. Restart interval was 64 seconds. Existing signed history is unchanged.

New ordinary single-input transfers allocate 2,660 sats without change or 3,990
with change (33.5% below 4,000/6,000). Lightning reserves are unchanged. Older
wallets keep the legacy allocation; updated wallets negotiate support and
persist the selected budget for retries. Do not downgrade with pending transfers.
