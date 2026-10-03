# Recovery costs and low-cost transfers

## Legacy transfer budget

An Ark transfer has no separate service fee. It still deducts 4,000 sats per
input with one output, or 6,000 sats per input with recipient and change outputs.
The recipient receives the requested amount. Fragmented balances can cost more.
These are recovery allocations, not refundable deposits or immediate miner fees.
Refresh does not credit old allocations back. The server can reclaim unused
backing under the expiry rules. Expiry is measured in blocks, not a fixed month.

For a 10,000-sat Lightning receive with service pricing of 100 sats + 0.2%:
service fee = 120 sats, single-input recovery allocation = 4,000 sats,
and spendable amount = 5,880 sats. Additional HTLC inputs increase recovery cost.
This example describes Paperclip pricing, not a protocol-wide fixed service fee.

## Reduction work

First prefer a valid single input over fragmented inputs. Compare all valid single
inputs and prefer an exact spend with a 4,000-sat allocation over a change-producing
spend with a 6,000-sat allocation. Equal-cost inputs retain expiry order. Use the actual package
builder for both estimates and sends, including exact one-output spends. Retain
all admission, expiry, dust, and final-claim checks. This reduces avoidable costs;
it does not lower the 4,000-sat minimum of the current signed profile.

A smaller reserve must be based on transaction weight and relay requirements,
not payment value. Removing a checkpoint or sharing a reserve across inputs can
change recovery and double-spend protection. Neither is a configuration tweak.

A future lower-cost profile needs version negotiation, weight-based parent fees,
dust-safe anchors, sufficient final-claim funding, and full exit/reorg tests.
Existing signed VTXOs retain their original budgets. Migrate by cooperative refresh.

Refunds need a separate liability ledger. A reserve must not be credited while an
old recovery path can still consume its backing. The ledger must prevent duplicate
credits across transfers, refreshes, retries, and reorgs. Credit only after safe
reclamation, or fund an explicit operator subsidy independently. Do not promise a
refund before this design is implemented and tested.

## Display rules

Show recipient amount, service fee, recovery allocation, total debit and net
received before confirmation. State that allocations are not separately refundable.
Never label the whole allocation a miner fee. Link current capability documentation
from historical test reports; do not present old restrictions as current behavior.

## Negotiated small-anchor transfers

New servers advertise `small_anchor_transfers`. Updated wallets use 330-sat
standard P2WSH anchors and retain the 1,000-sat miner fee for ordinary Ark sends.
One input costs 2,660 sats without change or 3,990 sats with change. The sender
funds the entire allocation; the ASP does not subsidize the transfer.

The wallet uses the same builder for estimates and sends. It persists the
selected budget in the action before cosigning. Retries keep that budget even
if server capabilities change. Old action records default to the legacy budget.
Do not downgrade a wallet with pending small-anchor actions to an older binary.

This capability does not change the version-2 recovery encoding, old signed
ancestors, round funding, boarding, Lightning contracts, or offboard preparation.
Old clients can continue to request the legacy budget. An old server does not
advertise this capability, so an updated wallet uses the legacy budget there.
A deployment needs both server support and an updated sending wallet.
