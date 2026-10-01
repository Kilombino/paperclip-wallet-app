# On-chain message signing

The development wallet supports signing and verifying messages with its own
BIP86 Taproot on-chain addresses. In **On-chain → Sign a message**, enter an
existing wallet address and the complete challenge, review it, and copy the
signature. The wallet can sign with no balance and without contacting the ASP.
Signing does not broadcast a transaction or change the wallet balance.

The message is exact UTF-8, including spaces and line breaks (maximum 4096
bytes). The output is **BIP322-simple**, with the `smp` prefix specified by
[BIP322](https://bips.dev/322/). Verification also accepts historical unprefixed
simple proofs. Only Taproot key-path proofs using SIGHASH_DEFAULT or SIGHASH_ALL
are accepted. Legacy compact ECDSA `signmessage` proofs are not interchangeable.

This is a standard Bitcoin address-key ownership proof, deliberately separate
from XBT's unified transaction signatures. It proves control of the address key;
it does not identify the network, prove a balance, or authorize a payment by
itself. Real XBT transaction signing is unchanged. Pool verification must support
this proof format rather than relying on Knots' legacy `verifymessage` RPC.

## CLI and authenticated API

```sh
paperclip-wallet onchain sign-message 'bc1p...' 'exact challenge'
paperclip-wallet onchain verify-message 'bc1p...' 'exact challenge' 'smp...'
```

`POST /api/v1/onchain/message/sign` takes `address` and `message`. It returns
`address`, `message`, `signature`, and `scheme: "bip322-simple"`.

`POST /api/v1/onchain/message/verify` takes `address`, `message`, and `signature`.
It returns `valid: true` or `false`; malformed or unsupported proofs return 400.
Both endpoints require the wallet's existing authentication. The signing
endpoint only accepts addresses known to and owned by this wallet. Neither
endpoint accepts arbitrary transactions or PSBTs.

## Future pool payout verification

The pool should issue a single-use, expiring challenge that binds its domain,
the XBT network, the on-chain payout address, and the exact Lightning payout
destination. Include an unpredictable nonce and an explicit purpose such as
“Change pool payout destination”. Verify all fields and consume the nonce after
success. Never accept a generic reused ownership signature as authorization to
change payouts. The pool authorization flow is not implemented by this feature.

This software remains beta and unaudited.

## Validation

Run `just checks` and `just unit-wallet onchain_message` inside the Nix shell.
The tests verify a published Bitcoin BIP322 Taproot vector, signing from an empty
BIP86 wallet, exact UTF-8/whitespace binding, wrong addresses, and input bounds.
`node scripts/test-web.mjs` covers review cancellation, request authentication,
edited-message and locked-session invalidation, and clearing proofs on lock.
