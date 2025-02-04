pub mod assets;
pub mod commands;
pub mod feeder;
pub mod jup;
pub mod ray;
pub mod runner;
pub mod token_registry;
pub mod tray;

use commands::core::{greet, update_token_and_price};
use feeder::PriceInfo;
use jup::{format_price, TokenSymbol};
use runner::run_loop;
use tauri::{
    tray::TrayIconId, LogicalSize, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder,
};
use token_registry::{Token, TokenRegistry};
use tokio::sync::watch;
use tray::setup_tray;

use std::sync::Mutex;

#[allow(dead_code)]
#[derive(Clone)]
pub struct PriceTarget {
    token_or_pair_symbol: String,
    price: f64,
}

#[derive(Default)]
pub struct AppState {
    tray_id: Mutex<Option<TrayIconId>>,
    selected_tokens: Mutex<Vec<Token>>,
    token_sender: Mutex<Option<watch::Sender<Vec<Token>>>>,
    token_registry: Mutex<TokenRegistry>,
    is_quit: Mutex<bool>,
    price_targets: Mutex<Vec<PriceTarget>>,
    price_watches: Mutex<Vec<String>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .setup(|app| {
            let token_registry = TokenRegistry::new();
            let app_state = app.state::<AppState>();
            *app_state.token_registry.lock().unwrap() = token_registry;

            let tray_id = setup_tray(app.handle()).expect("Expect tray_id");
            *app_state.tray_id.lock().unwrap() = Some(tray_id.clone());

            let (token_sender, token_receiver) = watch::channel(vec![TokenRegistry::new()
                .get_by_symbol(&TokenSymbol::SOL)
                .expect("Token ot exist")
                .clone()]);
            *app_state.token_sender.lock().unwrap() = Some(token_sender);

            let (price_sender, price_receiver) = watch::channel::<PriceInfo>(PriceInfo::default());
            let app_handle = app.handle().clone();

            // Test
            let sol_symbol = TokenSymbol::SOL.to_string();
            let pair_symbol = format!("{}_{}", TokenSymbol::JLP, TokenSymbol::SOL);
            let price_targets = vec![
                PriceTarget {
                    token_or_pair_symbol: sol_symbol.clone(),
                    price: 200f64,
                },
                PriceTarget {
                    token_or_pair_symbol: pair_symbol.clone(),
                    price: 0.021f64,
                },
            ];
            *app_state.price_targets.lock().unwrap() = price_targets.clone();

            let price_watches = vec![sol_symbol, pair_symbol];
            *app_state.price_watches.lock().unwrap() = price_watches.clone();

            // Price effect
            tauri::async_runtime::spawn(async move {
                let mut price_receiver = price_receiver.clone();
                loop {
                    let _ = price_receiver.changed().await;
                    let price_info = *price_receiver.borrow_and_update();

                    let tray_icon = app_handle.tray_by_id(&tray_id).expect("Tray missing");

                    let price = price_info.price;
                    if let Some(price) = price {
                        // Update view
                        let _ = tray_icon.set_title(Some(format_price(price)));

                        // Notifications
                        price_targets.iter().for_each(|price_target| {
                            // Payload
                            if price_watches.contains(&price_target.token_or_pair_symbol)
                                && (price_target.price - price).abs() < 0.1f64
                            {
                                // TODO: Add to notify list
                                // TODO: Mark as notified by remove from price_targets
                            }
                        });
                    } else if price_info.retry_count > 0 {
                        // Update view
                        let _ = tray_icon.set_title(Some("…".to_owned()));
                    }
                }
            });

            // // Notify
            // app.notification()
            // .builder()
            // .title(format!(
            //     "{}: ${}",
            //     price_target.token_or_pair_symbol,
            //     format_price(price)
            // ))
            // // .body(format!("${}", format_price(price)))
            // .show()
            // .unwrap();

            tauri::async_runtime::spawn(async move {
                if let Err(e) = run_loop(price_sender, token_receiver).await {
                    eprintln!("Price fetch error: {}", e);
                }
            });

            Ok(())
        })
        .on_menu_event(|app_handle, event| {
            let id = event.id.as_ref();
            let state = app_handle.state::<AppState>();
            let token_registry = state.token_registry.lock().unwrap();

            match id {
                "settings" => {
                    let window = app_handle.get_webview_window("main");

                    let window = match window {
                        Some(window) => window,
                        None => tauri::WebviewWindowBuilder::new(
                            app_handle,
                            "Settings",
                            WebviewUrl::App("index.html".into()),
                        )
                        .title("Settings")
                        .always_on_top(true)
                        .build()
                        .unwrap(),
                    };

                    let _ = window.set_size(LogicalSize::new(360, 600));

                    window.show().unwrap();
                    window.set_focus().unwrap();
                }
                "quit" => {
                    *app_handle.state::<AppState>().is_quit.lock().unwrap() = true;
                    app_handle.exit(0);
                }
                "portfolio" => {
                    let window = app_handle.get_webview_window("portfolio");

                    let window = match window {
                        Some(window) => window,
                        None => WebviewWindowBuilder::new(
                            app_handle,
                            "portfolio",
                            WebviewUrl::External(
                                Url::parse("https://portfolio.jup.ag/").expect("Invalid url"),
                            ),
                        )
                        .always_on_top(true)
                        .build()
                        .unwrap(),
                    };

                    let _ = window.set_size(LogicalSize::new(360, 600));

                    window.show().unwrap();
                    window.set_focus().unwrap();
                }
                _ => {
                    let app_handle = app_handle.clone();
                    if id.contains("_") {
                        let pairs = id.split("_").collect::<Vec<_>>();
                        let tokens = vec![
                            token_registry
                                .get_by_address(pairs[0])
                                .expect("Not exist")
                                .clone(),
                            token_registry
                                .get_by_address(pairs[1])
                                .expect("Not exist")
                                .clone(),
                        ];

                        tauri::async_runtime::spawn(async move {
                            // Spawn a new async task
                            if let Err(e) = update_token_and_price(app_handle, tokens).await {
                                eprintln!("Error updating token and price: {}", e);
                            }
                        });
                    } else if let Some(token) = token_registry.get_by_address(id) {
                        let token = token.clone();
                        tauri::async_runtime::spawn(async move {
                            // Spawn a new async task
                            if let Err(e) = update_token_and_price(app_handle, vec![token]).await {
                                eprintln!("Error updating token and price: {}", e);
                            }
                        });
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![greet, update_token_and_price])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(move |app_handle, e| {
        if let RunEvent::ExitRequested { api, .. } = &e {
            // Keep the event loop running even if all windows are closed
            // This allow us to catch system tray events when there is no window
            if !*app_handle.state::<AppState>().is_quit.lock().unwrap() {
                api.prevent_exit();
            }
        }
    });
}
