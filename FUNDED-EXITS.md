# Funded recovery profile 2

This is a private experimental implementation for ordinary public-key balances.
Regtest is the default. Mainnet opt-in and its prerequisites are in MAINNET.md.

The wallet requires `ArkInfo.exit_profile = 2`. A board request includes the
same marker. Transfer requests include explicit recovery reserves. The wallet
checks the reserve profile in a round proposal before it signs. Legacy signed
graphs keep their original encoding and signatures; they do not become funded
through a software update. Cooperative refresh is the migration path.

## Reserves and balances

Each new recovery transaction pays 1,000 sats to miners and creates a 1,000-sat
P2WSH OP_TRUE anchor. These are separate values in the signed graph. The anchor is public:
anyone can spend it. It is not a protected wallet balance or a guaranteed future
fee-bump reserve. The parent can relay without an anchor child.

- Boarding deducts the greater of the quoted board fee or 1,000 sats for the
  anchor, plus a separate 1,000-sat miner fee. The CLI movement records the total.
- A checkpointed transfer with one recipient and one change output reserves
  6,000 sats from the sender. A transfer with one output reserves 4,000 sats.
  The recipient gets the requested amount. Each selected input has its own
  recovery transactions and reserves.
- A refresh preserves the amounts in the agreed output requests, less the
  existing refresh fee. The server funds the additional tree reserves from its
  round wallet. Operators must budget for this subsidy.
- Reserves are excluded from spendable balances. They are not separately
  refundable deposits. They are consumed by the recovery graph if it is used;
  cooperative settlement follows the existing Ark reclaim rules.

Outputs must leave at least 330 sats after a 1,000-sat final-claim allowance.
Payments or change below 1,330 sats are refused. Some fragmented input sets
require a refresh before payment. No recipient amount is silently reduced to
make an allocation fit.

## Recovery

Keep a private backup of the complete wallet directory. It includes the signed
recovery graph, not just the seed. Start an exit through the wallet CLI before
the expiry deadline. The wallet broadcasts funded parents after their inputs
confirm, then waits for the relative timelock before claiming the output.
The server does not need to be online or sign again.

Parent status is checked again after a restart, eviction, or reorganization.
Transactions retained only in a stale block are checked against the actual
mempool before suppressing rebroadcast. A reorganization invalidates the previous
claimable state and recalculates confirmation and relative-locktime requirements.
The existing CPFP interface can add fees using separate confirmed wallet coins.
The wallet never changes a signed parent to increase its fee. A parent that
already pays the selected rate does not need an additional wallet-funded child.

Admission checks cover signatures, per-parent miner fees, dust, claim allowance,
and the remaining time for the chain of confirmations and relative timelock.
Transfers reserve time for their two new ancestors. Expiry remains part of the
protocol. No finite reserve guarantees confirmation under arbitrary fees,
censorship, or a chain halt. An offline wallet needs an operator to resume it
and exit before its deadline.

## Policy and scope

The target is Knots v29.4.2.knots20260508 with standardness enabled on regtest
and ordinary version-2 relay rules, including `mempooltruc=reject`. The test runner does not set `mempooltruc=enforce` or
`subdustfeepenalty=0`. Wallet admission requires a Knots RPC backend and refuses
new positions when reported relay, mempool, or dust fees exceed the tested
envelope. Recovery of existing positions remains available separately.

Unified ALL signatures remain mandatory. Mainnet requires a separate explicit opt-in; see MAINNET.md.
Lightning HTLCs and server liquidity-pool allocation are outside this profile;
the server rejects configurations with a CLN backend. Use an empty VTXO pool
for these tests. A separate review and test program is required for those paths.

See the companion ASP's `scripts/test-default-policy.sh` and validation report
for the exact checks that passed. A successful standalone transaction test is
not evidence that the entire wallet lifecycle passed.

## Versioned compatibility

Profile 2 creates transaction version 2 and uses a standard SegWit v0 fee anchor.
It requires no P2A or TRUC support. Unified XBT signatures still carry 0x21.
The public anchor is not a protected user balance. It can be spent by anyone.

VTXO encoding 4 and tree encoding 4 explicitly identify the signed exit format.
Decoders retain versions 1–3. Old data reconstructs the original version-3 P2A
transactions exactly; it is never reinterpreted as version 2. New clients reject
a mismatched ASP profile before boarding. Old positions require cooperative
refresh to gain the new format; direct transfers cannot mix recovery profiles.

The ASP's watchman requires matching version-2 protection transactions. These
protect the ASP from someone withdrawing or refreshing a balance and then trying
to exit an old copy. Normal receipt amounts and agreed withdrawals are unchanged.

Regtest validation and mainnet pilot validation are separate. The earlier tiny
mainnet pilot used a user-controlled anchor and is not a full ASP lifecycle test.
