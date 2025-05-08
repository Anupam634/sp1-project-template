use axum::{extract::Json, http::StatusCode, routing::post, Router};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sp1_sdk::{include_elf, HashableKey, ProverClient, SP1Stdin};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tokio::task;
use std::convert::TryInto;

pub const FIBONACCI_ELF: &[u8] = include_elf!("fibonacci-program");

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long)]
    execute: bool,

    #[arg(long)]
    prove: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserData {
    id: u32,
    user_address: String,
    amount_in_btc: f64,
    price_at_deposited: String,
    usbd_minted: String,
    collateral_ratio: String,
}

#[derive(Serialize, Debug)]
pub struct ProofResponse {
    proof: String,
    icr: u32,
    collateral_amount_usd: u32,
    vkey: String,
}

async fn fetch_btc_price() -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
    println!("Attempting to fetch BTC price from Coindesk...");
    let url = "https://api.coindesk.com/v1/bpi/currentprice/BTC.json";

    match reqwest::get(url).await {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await?;
            if let Some(price) = json["bpi"]["USD"]["rate_float"].as_f64() {
                return Ok((price * 100.0) as u32); // USD cents
            }
            Err("Failed to parse BTC price".into())
        }
        Err(e) => {
            println!("Failed to fetch BTC price, using fallback: {e}");
            Ok(6200000) // fallback = $62,000.00 in cents
        }
    }
}

async fn fetch_user_data(user_id: u32) -> Result<UserData, Box<dyn std::error::Error + Send + Sync>> {
    Ok(UserData {
        id: user_id,
        user_address: "user_btc_address".to_string(),
        amount_in_btc: 0.01,
        price_at_deposited: "2025-05-07T17:57:00Z".to_string(),
        usbd_minted: "300".to_string(),
        collateral_ratio: "200".to_string(),
    })
}

fn write_inputs_to_stdin(stdin: &mut SP1Stdin, user_data: &UserData, btc_price: u32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sats = (user_data.amount_in_btc * 100_000_000.0) as u32;
    let debt = user_data.usbd_minted.parse::<u32>()?;

    stdin.write(&user_data.id);                     // u32
    stdin.write(&user_data.user_address);           // String
    stdin.write(&user_data.price_at_deposited);     // String (created_at)
    stdin.write(&sats);                             // u32 (collateral_amount)
    stdin.write(&debt);                             // u32 (usbd_minted)
    stdin.write(&btc_price);                        // u32 (btc_price_usd)

    Ok(())
}

fn parse_public_values(values: &[u8]) -> Result<(u32, u32), StatusCode> {
    if values.len() < 8 {
        return Err(StatusCode::INTERNAL_SERVER_ERROR); // Ensure there are enough bytes to represent 2 u32s
    }

    // Convert the byte slice to two u32 values
    let icr = u32::from_le_bytes(values[0..4].try_into().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    let collateral_amount_usd = u32::from_le_bytes(values[4..8].try_into().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);

    Ok((icr, collateral_amount_usd))
}

async fn prove_icr(Json(input): Json<UserData>) -> Result<Json<ProofResponse>, StatusCode> {
    println!("Received request: {:?}", input);

    let btc_price = fetch_btc_price().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let user_data = input.clone();

    println!("Starting proof generation...");

    let result = task::spawn_blocking(move || {
        let client = ProverClient::from_env();
        let mut stdin = SP1Stdin::new();
        write_inputs_to_stdin(&mut stdin, &user_data, btc_price)?;

        let (pk, vk) = client.setup(FIBONACCI_ELF);
        let proof = client.prove(&pk, &stdin).groth16().run()?;

        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((proof.bytes().to_vec(), proof.public_values.to_vec(), vk.bytes32()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        Ok((proof_bytes, public_values, vkey)) => {
            let (icr, collateral_amount_usd) = parse_public_values(&public_values)?;

            Ok(Json(ProofResponse {
                proof: hex::encode(proof_bytes),
                icr,
                collateral_amount_usd,
                vkey,
            }))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.execute {
        println!("Running in execute mode...");

        let user_data = fetch_user_data(1).await.expect("Failed to fetch user data");
        let btc_price = fetch_btc_price().await.expect("Failed to fetch BTC price");

        let result = task::spawn_blocking(move || {
            let client = ProverClient::from_env();
            let mut stdin = SP1Stdin::new();
            write_inputs_to_stdin(&mut stdin, &user_data, btc_price)?;

            let (pk, vk) = client.setup(FIBONACCI_ELF);
            let proof = client.prove(&pk, &stdin).groth16().run()?;

            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((proof.bytes().to_vec(), proof.public_values.to_vec(), vk.bytes32()))
        })
        .await
        .expect("Join failed")
        .expect("Proof generation failed");

        let (proof_bytes, public_values, vkey) = result;

        let (icr, collateral_amount_usd) = parse_public_values(&public_values).expect("Invalid public values");

        let response = ProofResponse {
            proof: hex::encode(proof_bytes),
            icr,
            collateral_amount_usd,
            vkey,
        };

        println!("Execution mode result:\n{:#?}", response);
        return;
    }

    if args.prove {
        let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any);
        let app = Router::new()
            .route("/prove_icr", post(prove_icr))
            .layer(cors);

        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        println!("Server running at http://{}", addr);
        axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
            .await
            .unwrap();
    }
}