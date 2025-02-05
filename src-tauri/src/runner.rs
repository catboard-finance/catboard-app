use std::collections::HashMap;

use anyhow::Result;

use tokio::sync::watch;
use tokio::time::{sleep, Duration};

use crate::feeder::PriceInfo;
use crate::jup::{fetch_pair_price, fetch_price};
use crate::token_registry::Token;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run_loop(
    price_sender: watch::Sender<HashMap<String, PriceInfo>>,
    token_receiver: watch::Receiver<Vec<Token>>,
) -> Result<()> {
    let mut tokens = token_receiver.borrow().clone();
    let mut retry_count = 0;

    loop {
        // Check for token changes
        if token_receiver.has_changed()? {
            tokens = token_receiver.borrow().clone();
            retry_count = 0; // Reset retry counter on token change
        }

        // Fetch price with retry logic
        let is_pair = tokens.len() == 2;

        println!(
            "Price fetch: {}",
            tokens
                .iter()
                .map(|e| e.symbol.to_string())
                .collect::<Vec<String>>()
                .join("_")
        );

        if !is_pair {
            let mut price_map = HashMap::new();
            let address = tokens[0].address.clone();
            match fetch_price(&address).await {
                Ok(price) => {
                    retry_count = 0; // Reset retry counter on success
                    price_map.insert(
                        address,
                        PriceInfo {
                            price: Some(price),
                            retry_count,
                        },
                    );
                    price_sender.send(price_map)?;
                }
                Err(e) => {
                    retry_count += 1;
                    println!("Price fetch failed (attempt {}): {}", retry_count, e);
                    price_map.insert(
                        address,
                        PriceInfo {
                            price: None,
                            retry_count,
                        },
                    );
                    price_sender.send(price_map)?;

                    // Exponential backoff up to 5 minutes
                    let backoff = Duration::from_secs(30).mul_f32(2f32.powi(retry_count - 1));
                    sleep(backoff.min(Duration::from_secs(300))).await;
                    continue;
                }
            }
        } else {
            let address = format!("{}_{}", tokens[0].address, tokens[1].address);
            let mut price_map = HashMap::new();

            match fetch_pair_price(&tokens[0].address, &tokens[1].address).await {
                Ok(price) => {
                    retry_count = 0; // Reset retry counter on success
                    price_map.insert(
                        address,
                        PriceInfo {
                            price: Some(price),
                            retry_count,
                        },
                    );
                    price_sender.send(price_map)?;
                }
                Err(e) => {
                    retry_count += 1;
                    println!("Price fetch failed (attempt {}): {}", retry_count, e);
                    price_map.insert(
                        address,
                        PriceInfo {
                            price: None,
                            retry_count,
                        },
                    );
                    price_sender.send(price_map)?;

                    // Exponential backoff up to 5 minutes
                    let backoff = Duration::from_secs(30).mul_f32(2f32.powi(retry_count - 1));
                    sleep(backoff.min(Duration::from_secs(300))).await;
                    continue;
                }
            }
        };

        // Wait for next poll
        sleep(POLL_INTERVAL).await;
    }
}
