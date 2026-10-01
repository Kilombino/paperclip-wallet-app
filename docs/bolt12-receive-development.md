# Reusable BOLT12 receive development

Status: incomplete; not enabled in the published wallet or server packages.

The target is a reusable BOLT12 offer. A single-use invoice is not a substitute.
Every payment must use a fresh wallet-owned preimage and settle through the Ark
HTLC claim process. The server must not collect a normal CLN offer payment and
credit a custodial balance in its place.

## Implemented groundwork

- Receive checkpoints and permanent settlement records accept BOLT11 and BOLT12
  invoices. Existing invoice strings and BOLT11 checkpoint JSON remain readable.
- Receive status and cancellation can identify a payment by either invoice format.
- Server subscription records accept both formats. Exact invoice matching remains
  required for internal payments; an identical hash alone is not sufficient.
- Wallet receive cards generate QR codes locally. Copy, native sharing where
  available, SVG download, mobile layout and reduced motion are supported.

No endpoint currently creates a reusable receive offer. Do not advertise this
branch as complete BOLT12 receive support, tag a release, or update package pins.

## Work required before release

1. Persist authenticated offer registration, revocation and per-payment request
   bindings. Bound requests and stored records to prevent resource exhaustion.
2. Route BOLT12 invoice requests through the hold plugin onion-message stream.
   Validate signed requests, offer ownership, XBT identity, network, amount and
   expiry before allocating a payment hash. Preserve unrelated CLN offer traffic.
3. Persist wallet-owned secrets before making invoices payable. Allocate a fresh
   secret per payment; never reuse a settled, expired or canceled payment hash.
   If pre-provisioned hashes are used for offline invoice issuance, consumption
   must be atomic and durable, with bounded refill and safe exhaustion behavior.
4. Build and sign matching BOLT12 invoices and register hold state plus the Ark
   subscription before returning an invoice. The wallet must verify that each
   invoice matches its hash, offer, amount, XBT identity and required CLTV window.
5. Resume requests, hold reconciliation and Ark claims after wallet/server restart.
   Cancellation must not lose a pending claim or disclose a secret before the
   wallet obtains recoverable HTLC VTXOs. Preserve settlement secrets for exits.
6. Add offer creation, copy/QR, status and disable controls with clear online and
   offline limits. Keep the reusable offer separate from its payment activity.
7. Test two payments to the same offer with different hashes, concurrent requests,
   duplicate/replayed requests, retries, expiry, revocation, depleted liquidity,
   restarts and recovery. Run a complete CLN-to-Ark settlement in isolated regtest.
8. Only after these checks, enable the server capability, build both image
   architectures, test Umbrel, sign StartOS 0.4 packages, update store pins and
   publish matching source/release documentation. Old clients must not receive
   a BOLT12 response in a field documented as BOLT11.

## Validation performed on the groundwork

The QR test supports an independent jsQR decoder supplied as its CLI argument.
Browser tests cover actual rendered scan round trips, mobile overflow, reduced
motion and clearing QR images on lock. Rust tests cover legacy checkpoint JSON,
BOLT12 checkpoint/settlement persistence, payment hash validation and exact
server invoice matching. These checks are not end-to-end BOLT12 receive tests
and are not an independent security audit.
