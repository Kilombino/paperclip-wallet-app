# Recovery costs in 0.9.0

Ordinary Ark sends can use a smaller recovery budget when the server advertises
support. For one input, the allocation is 1,056 sats without change or 1,584 sats
with change. Multiple inputs require additional recovery transactions. These
amounts are recovery allocations, not an ASP service fee or a promise that every
payment costs the same amount.

Each new ordinary recovery transaction reserves a 330-sat anchor and a 198-sat
miner fee. The exception applies only to checkpointed pubkey transfers with at
most two outputs. Tree and Lightning recovery budgets are unchanged. Existing
signed transactions remain unchanged. Servers without the capability use the
previous budget.

Tests covered a funded send, change, an exact-balance send, spending with the ASP
offline, and a user-funded 20 sat/vB fee bump followed by claiming the exit.
Compatibility testing included 0.8.3 receiving from the new wallet, sending back,
and exiting with the ASP offline. Keep a complete wallet backup; this does not
authorize downgrading a wallet with pending operations.

## Why P2A is not enabled

On Knots 29.4.3.knots20260508 with its default relay policy, isolated transaction
tests accepted and mined version-2 P2A graphs with 240-sat anchors. The measured
allocation was 730 sats without change and 1,138 sats with change. Fee-bump tests
also accepted an anchor child funded with the user's own on-chain input. These
were transaction-policy probes, not a complete Ark wallet implementation.

The existing signed formats identify version-3 P2A or version-2 P2WSH. Changing
either definition changes signed transaction history. A new version-2 P2A tag
would be rejected by previous wallets. Tests explicitly preserve those format
definitions and reject replacing a signed standard ancestor with P2A.

P2A therefore remains excluded from 0.9.0. It needs recipient capability
negotiation, a supported transition for existing funds, persistence and restore
tests, and complete offline wallet-exit tests before it can replace this budget.
The primitive policy results alone do not establish compatibility or exit safety.
