# Lightning selection regression

`lightning-selection.rs` is an isolated XBT regtest test for the ASP integration
harness. Copy it to `testing/tests/bark/xbt.rs` in the harness, using this wallet
source for the harness's wallet crates and the built wallet CLI as `BARK_EXEC`.
Run `just int "--test bark xbt_lightning_constructible_selection --test-threads 1"`.
Never run integration tests directly with `cargo test`.

It boards 45,800, 10,000, and 84,805 sats into separate inputs, estimates a
50,000-sat payment, checks that one usable input is selected, and pays both a
BOLT11 invoice and a BOLT12 offer. The final balance must match the estimate.
The lower minimum board amount is restricted to this isolated test fixture.
An unaffordable estimate must fail without consuming inputs.

Wallet unit coverage: `just unit-wallet constructible_`.
Web preview coverage: `node scripts/test-web.mjs`.

These checks and `just checks` passed for the 0.8.1 fix on XBT regtest.
No live payment is needed for the upgrade's read-only estimate verification.
