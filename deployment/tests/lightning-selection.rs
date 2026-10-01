//! Copy into the ASP integration harness as testing/tests/bark/xbt.rs; run with just int.
use ark_testing::{btc, sat, TestContext};

#[tokio::test]
async fn xbt_lightning_constructible_selection() {
	let ctx = TestContext::new("xbt/lightning-selection").await;
	ctx.generate_blocks(200).await;
	let ln = ctx.new_lightning_setup("ln").await;
	let srv = ctx.captaind("asp").lightningd(&ln.internal).funded(btc(2))
		.cfg(|c| {
			c.experimental_funded_lightning = true;
			// Reproduce a small received/change VTXO using a direct regtest board.
			c.min_board_amount = sat(10_000);
		}).create().await;
	for bolt12 in [false, true] {
		let name = if bolt12 { "bolt12" } else { "bolt11" };
		let wallet = ctx.bark(name, &srv).funded(sat(500_000)).create().await;
		for amount in [45_800, 10_000, 84_805] {
			wallet.board_and_confirm_and_register(&ctx, sat(amount)).await;
		}
		let client = wallet.client().await;
		let before = client.spendable_vtxos().await.unwrap();
		let quote = client.estimate_lightning_send_fee(sat(50_000)).await.unwrap();
		assert_eq!(quote.vtxos_spent.len(), 1);
		let selected = before.iter().find(|v| v.id() == quote.vtxos_spent[0]).unwrap();
		assert!(selected.amount() > sat(80_000));
		assert_eq!(before.len(), client.spendable_vtxos().await.unwrap().len());
		assert!(client.estimate_lightning_send_fee(sat(500_000)).await.is_err());
		assert_eq!(before.len(), client.spendable_vtxos().await.unwrap().len());
		let balance = wallet.spendable_balance().await;
		drop(client);
		let destination = if bolt12 {
			ln.external.offer(None, Some("fragmented funds regression")).await
		} else {
			ln.external.invoice(Some(sat(50_000)), name, "fragmented funds regression").await
		};
		wallet.pay_lightning_wait(&destination, if bolt12 { Some(sat(50_000)) } else { None }).await;
		assert_eq!(wallet.spendable_balance().await, balance - quote.gross_amount);
	}
}
