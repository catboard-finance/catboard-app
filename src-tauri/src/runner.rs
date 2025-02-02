use anyhow::Result;

use tokio::sync::watch;
use tokio::time::{sleep, Duration};

use crate::jup::{fetch_pair_price, fetch_price};
use crate::token_registry::Token;

const POLL_INTERVAL: Duration = Duration::from_secs(30);

pub async fn run_loop(
    price_sender: watch::Sender<Option<f64>>,
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
        if !is_pair {
            match fetch_price(&tokens[0].address).await {
                Ok(price) => {
                    price_sender.send(Some(price))?;
                    retry_count = 0; // Reset retry counter on success
                }
                Err(e) => {
                    retry_count += 1;
                    println!("Price fetch failed (attempt {}): {}", retry_count, e);

                    // Exponential backoff up to 5 minutes
                    let backoff = Duration::from_secs(30).mul_f32(2f32.powi(retry_count - 1));
                    sleep(backoff.min(Duration::from_secs(300))).await;
                    continue;
                }
            }
        } else {
            match fetch_pair_price(&tokens[0].address, &tokens[1].address).await {
                Ok(price) => {
                    price_sender.send(Some(price))?;
                    retry_count = 0; // Reset retry counter on success
                }
                Err(e) => {
                    retry_count += 1;
                    println!("Price fetch failed (attempt {}): {}", retry_count, e);

                    // Exponential backoff up to 5 minutes
                    let backoff = Duration::from_secs(30).mul_f32(2f32.powi(retry_count - 1));
                    sleep(backoff.min(Duration::from_secs(300))).await;
                    continue;
                }
            }
        }

        // Wait for next poll
        sleep(POLL_INTERVAL).await;
    }
}
