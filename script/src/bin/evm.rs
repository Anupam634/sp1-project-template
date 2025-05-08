use alloy_sol_types::SolType;
use clap::{Parser, ValueEnum};
use dotenv::dotenv;
use fibonacci_lib::{PublicValuesStruct, PublicValuesIcr, PublicValuesLquidation, PublicValuesLtv};
use reqwest;
use serde::{Deserialize, Serialize};
use sp1_sdk::{
    include_elf, ProverClient, SP1Stdin, SP1VerifyingKey,
    HashableKey,
};
use std::path::PathBuf;

pub const FIBONACCI_ELF: &[u8] = include_elf!("fibonacci-program");

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct EVMArgs {
    #[arg(long, value_enum, default_value = "groth16")]
    system: ProofSystem,

    #[arg(long)]
    user_index: Option<usize>, // NEW: optional user index
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ProofSystem {
    Plonk,
    Groth16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SP1Fixture {
    user_id: u32,
    user_address: String,
    a: u32,
    b: u32,
    icr: u32,
    collateral_amount: u32,
    liquidation_threshold: u32,
    real_time_ltv: u32,
    n: u32,
    vkey: String,
    public_values: String,
    proof: String,
}

#[derive(Debug, Deserialize)]
struct BtcPriceResponse {
    bitcoin: BtcPrice,
}

#[derive(Debug, Deserialize)]
struct BtcPrice {
    usd: f64,
}

#[derive(Debug, Deserialize, Clone)]
struct UserData {
    id: u32,
    user_address: String,
    amount_in_btc: f64,
    price_at_deposited: String,
    usbd_minted: String,
    collateral_ratio: String,
    created_at: String,
}

#[tokio::main]
async fn main() {
    sp1_sdk::utils::setup_logger();
    dotenv().ok();

    let args = EVMArgs::parse();
    let client = ProverClient::from_env();

    let btc_price_usd = fetch_btc_price().await.expect("Failed to fetch BTC price");
    let users = fetch_data_from_api().await.expect("Failed to fetch user data");

    if users.is_empty() {
        eprintln!("No user data found.");
        return;
    }

    let selected_users: Vec<UserData> = match args.user_index {
        Some(index) => {
            if index >= users.len() {
                eprintln!("❌ Invalid user index. Max allowed: {}", users.len() - 1);
                return;
            }
            vec![users[index].clone()]
        }
        None => users.clone(),
    };

    let (pk, vk) = client.setup(FIBONACCI_ELF);

    for user in selected_users {
        let amount_in_btc = user.amount_in_btc;
        let price_at_deposited = user.price_at_deposited.parse::<f64>().unwrap_or_default();
        let collateral_amount = ((amount_in_btc * price_at_deposited).round()) as u32;
        let usbd_minted = user.usbd_minted.parse::<u32>().unwrap_or_default();
        let btc_balance = (amount_in_btc.round()) as u32;

        let mut stdin = SP1Stdin::new();
        let n = 20;

        stdin.write(&n);
        stdin.write(&collateral_amount);
        stdin.write(&usbd_minted);
        stdin.write(&btc_price_usd);
        stdin.write(&usbd_minted);
        stdin.write(&btc_balance);
        stdin.write(&user.id);
        stdin.write(&user.user_address);
        stdin.write(&user.created_at);

        let proof = match args.system {
            ProofSystem::Plonk => client.prove(&pk, &stdin).plonk().run(),
            ProofSystem::Groth16 => client.prove(&pk, &stdin).groth16().run(),
        }
        .expect("failed to generate proof");

        let bytes = proof.public_values.as_slice();
        let decoded = PublicValuesStruct::abi_decode(bytes).unwrap();
        let decoded2 = PublicValuesIcr::abi_decode(bytes).unwrap();
        let decoded3 = PublicValuesLquidation::abi_decode(bytes).unwrap();
        let decoded4 = PublicValuesLtv::abi_decode(bytes).unwrap();
        let fixture = SP1Fixture {
            user_id: user.id,
            user_address: user.user_address.clone(),
            a: decoded.a,
            b: decoded.b,
            icr: decoded2.icr,
            collateral_amount: decoded2.collateral_amount,
            liquidation_threshold: decoded3.liquidation_threshold,
            real_time_ltv: decoded4.real_time_ltv,
            n: decoded.n,
            vkey: vk.bytes32().to_string(),
            public_values: format!("0x{}", hex::encode(bytes)),
            proof: format!("0x{}", hex::encode(proof.bytes())),
        };

        // Only print liquidationThreshold if a specific user was requested
        if args.user_index.is_some() {
            println!(
                "User ID: {} | liquidationThreshold: {}",
                fixture.user_id, fixture.liquidation_threshold
            );
        }

        let safe_address = user.user_address.replace(":", "_");
        let file_name = format!("{}-proof.json", safe_address);
        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../contracts/src/fixtures")
            .join(file_name);

        std::fs::create_dir_all(fixture_path.parent().unwrap()).unwrap();
        std::fs::write(&fixture_path, serde_json::to_string_pretty(&fixture).unwrap())
            .expect("failed to write fixture");

        println!("✅ Fixture saved for user: {}", user.user_address);
    }
}

async fn fetch_btc_price() -> Result<u32, Box<dyn std::error::Error>> {
    let url = "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd";
    let resp: BtcPriceResponse = reqwest::get(url).await?.json().await?;
    Ok(resp.bitcoin.usd.round() as u32)
}

async fn fetch_data_from_api() -> Result<Vec<UserData>, Box<dyn std::error::Error>> {
    let url = "http://139.59.8.108:3010/service/users-data";
    let resp = reqwest::get(url).await?.json::<Vec<UserData>>().await?;
    Ok(resp)
}