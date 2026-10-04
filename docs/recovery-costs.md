# Recovery costs and low-cost transfers

## Current ordinary Ark transfers

Paperclip ASP advertises `small_anchor_transfers`. Wallet 0.8.2 uses 330-sat
standard P2WSH anchors and retains the 1,000-sat miner fee for new ordinary Ark
transfers. One input allocates **2,660 sats without change** or **3,990 sats with
change**. There is no separate Ark-send service fee. The recipient receives the
requested amount and the sender funds the allocation; the ASP does not subsidize
it. Multiple inputs can cost more. The actual estimate is authoritative.

The wallet prefers a valid single input and compares constructible selections
using the same builder for estimates and sends. Exact spends can avoid a change
output. Admission, expiry, dust, and final-claim checks remain enforced.

## Legacy and Lightning budgets

Older wallets, older saved actions, and wallets connected to servers without the
capability retain the legacy 4,000-sat allocation per input without change, or
6,000 sats with recipient and change. Existing signed recovery paths are unchanged.
The lower budget applies to new ordinary transfers; it does not reprice historical
allocations or require rewriting old VTXOs.

Lightning contracts, round funding, boarding, and offboard preparation retain their
existing funding. A 10,000-sat Lightning receive with service pricing of 100 sats +
0.2% deducts 120 sats in service fees and 4,000 sats for a single HTLC input, leaving
5,880 spendable sats. More HTLC inputs can increase that allocation. This describes
Paperclip pricing, not a protocol-wide fixed service fee.

## What the allocation pays for

Recovery allocations are not refundable deposits or immediate miner fees.
Refresh does not credit previous allocations back. The server can reclaim unused
backing under the expiry rules, measured in blocks rather than a fixed month.
A smaller reserve must follow transaction weight and relay requirements, not
payment value. Removing checkpoints or sharing reserves across inputs changes
recovery and double-spend protection and needs a separate protocol design.

Refunds require a liability ledger that prevents double credits and does not
credit funds while an old recovery path can still consume its backing. No refund
or operator-funded subsidy is implemented or promised.

## Compatibility and recovery

The additive capability preserves version-2 recovery encoding and old signed
ancestors. Old clients can request the legacy budget; new wallets fall back when
an older server does not advertise support. An updated server and sending wallet
are both required to obtain the reduction.

The wallet persists its selected budget before cosigning. Retries retain it even
if server capabilities change; old saved actions default to the legacy budget.
Do not downgrade a wallet with pending small-anchor actions. Back up the complete
wallet before updating. See [release validation](small-anchor-validation.md) for
exit, reorg, relay, watchman and old-wallet checks and the test-suite limitations.

## Display rules

Show recipient amount, service fee, recovery allocation, total debit and net
received before confirmation. Never label the whole allocation a miner fee or
promise a separate refund. Historical test reports are not current fee quotes.
