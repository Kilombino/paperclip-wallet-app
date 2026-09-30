//! Explicit network scope for the experimental Paperclip application.
use bitcoin::Network;

/// This gate does not replace backend network, activation or index checks.
pub fn enabled(network: Network) -> bool {
	allowed(network, std::env::var("PAPERCLIP_XBT_MAINNET").as_deref() == Ok("1"))
}

fn allowed(network: Network, opt_in: bool) -> bool {
	match network {
		Network::Regtest => true,
		Network::Bitcoin => opt_in,
		_ => false,
	}
}

#[cfg(test)]
mod test {
	use super::*;
	#[test]
	fn mainnet_requires_explicit_opt_in() {
		assert!(allowed(Network::Regtest, false));
		assert!(!allowed(Network::Bitcoin, false));
		assert!(allowed(Network::Bitcoin, true));
		for network in [Network::Testnet, Network::Testnet4, Network::Signet] {
			assert!(!allowed(network, false));
			assert!(!allowed(network, true));
		}
	}
}
