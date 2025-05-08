#![no_main]
sp1_zkvm::entrypoint!(main);

use fibonacci_lib::calculate_icr;
use sp1_zkvm::io;

pub fn main() {
    // Read inputs from the prover
    let id = io::read::<u32>();
    let user_address = io::read::<String>();
    let created_at = io::read::<String>();
    let collateral_amount = io::read::<u32>();
    let debt_amount = io::read::<u32>();
    let btc_price_usd = io::read::<u32>();

    // Compute ICR and USD value
    let (icr, collateral_amount_usd) = calculate_icr(
        id,
        user_address,
        created_at,
        collateral_amount,
        debt_amount,
        btc_price_usd,
    );

    // Write public outputs (as single values)
    io::write(icr as u32, b"icr");
    io::write(collateral_amount_usd as u32, b"collateral_usd");
}