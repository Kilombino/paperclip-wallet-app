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
credit funds while an old recovery path can still consume its backing. The failed-Lightning candidate below adds a separate ASP-funded subsidy; it does
not release or reuse backing from an existing recovery path.

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

## Candidate: failed Lightning sends

The `fix/lightning-failed-payment-costs` branch is not a deployed release.
Updated wallets send invoice and amount preflight before the ASP signs the HTLC.
Expired or nearly expired invoices are rejected. Public BOLT11 invoices without
route hints also receive a route check within the existing safe delay and fee
limits. This cannot guarantee routing success or fully inspect private/blinded
routes.

After an eligible failure before dispatch, the ASP can compensate the actual
recorded setup and revocation costs using its own funded VTXOs. Eligibility is
limited to verified local rejections with no CLN send records, or expiration
before any attempt. The grant and its pool spend commit atomically and retries
return the same signed grant. Principal recovery remains available if ASP
liquidity cannot immediately cover compensation. Uncertain failures are not
automatically approved. The original recovery funding remains intact.

Older wallets may omit both preflight fields and continue using the existing
send and recovery protocol. Their invoices are checked at initiation, after the
HTLC has been signed, so they cannot receive the earlier route/expiry protection.
Eligible recovery costs are delivered as a separate ordinary Ark inbox payment,
using the original refund key and the sender inbox supplied at initiation. The
failed movement may still show its old cost; the separate receipt offsets it.
Wallets without an inbox retain a pending credit for operator reconciliation.

Grant, pool spend and inbox delivery commit together. The server retries queued
legacy credits when liquidity becomes available, including after restart. It
never removes exit reserves or increases Lightning's safe delay limit. Updated
wallets retain inline compensation and the awaiting-reimbursement state. Do not
downgrade wallets with pending reimbursement actions. Migrations V67/V68 do not
backfill historical costs or reimburse old incidents.

The REST send response says Lightning payment was initiated, not successful;
clients must check the final outcome using the returned payment hash.
