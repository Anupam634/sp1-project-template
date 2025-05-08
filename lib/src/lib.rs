use sp1_zkvm::io;

pub fn calculate_icr(
    id: u32,
    user_address: String,
    created_at: String,
    collateral_amount: u32,
    debt_amount: u32,
    btc_price_usd: u32,
) -> (u32, u32) {
    // Here you can calculate the Initial Collateral Ratio (ICR)
    // and the collateral amount in USD.
    let icr = (collateral_amount as f64 / debt_amount as f64) * 100.0;
    let collateral_amount_usd = (collateral_amount as f64 * btc_price_usd as f64) / 100_000_000.0;
    
    // Return ICR and collateral amount in USD
    (icr as u32, collateral_amount_usd as u32)
}