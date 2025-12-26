/// Manages WebSocket connections to multiple exchanges with auto-reconnect

use super::{ExchangeConnector, MarketMessage};
use crate::metrics::SharedMetrics;
use crate::orderbook::SharedOrderBookManager;
use crate::types::{ClientMessage, ORDERBOOK_DEPTH, ORDERBOOK_DISPLAY_DEPTH};
use futures_util::{SinkExt, StreamExt};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

/// Multi-Exchange Manager
///
/// Manages connections to multiple exchanges and unifies their market data streams
pub struct ExchangeManager {
    connectors: Vec<ExchangeConnector>,
    orderbook_manager: SharedOrderBookManager,
    metrics: SharedMetrics,
}

impl ExchangeManager {
    /// Create a new manager with multiple exchange connectors
    pub fn new(
        connectors: Vec<ExchangeConnector>,
        orderbook_manager: SharedOrderBookManager,
        metrics: SharedMetrics,
    ) -> Self {
        Self {
            connectors,
            orderbook_manager,
            metrics,
        }
    }

    /// Start all exchange connections (spawns one task per exchange)
    pub async fn start_all(
        &self,
        client_broadcast_tx: broadcast::Sender<ClientMessage>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = vec![];

        for connector in &self.connectors {
            let connector = connector.clone();
            let broadcast_tx = client_broadcast_tx.clone();
            let orderbook_manager = self.orderbook_manager.clone();
            let metrics = self.metrics.clone();

            let handle = tokio::spawn(async move {
                Self::run_exchange_connection(connector, broadcast_tx, orderbook_manager, metrics)
                    .await;
            });

            handles.push(handle);
        }

        handles
    }

    /// Run a single exchange connection with auto-reconnect
    async fn run_exchange_connection(
        connector: ExchangeConnector,
        client_broadcast_tx: broadcast::Sender<ClientMessage>,
        orderbook_manager: SharedOrderBookManager,
        metrics: SharedMetrics,
    ) {
        let exchange = connector.exchange();
        let exchange_name = exchange.name();

        loop {
            tracing::info!("[{}] Starting connection...", exchange_name);

            match Self::connect_and_process(
                connector.clone(),
                client_broadcast_tx.clone(),
                Arc::clone(&orderbook_manager),
                Arc::clone(&metrics),
            )
            .await
            {
                Ok(_) => {
                    tracing::info!("[{}] Connection closed gracefully", exchange_name);
                }
                Err(e) => {
                    tracing::error!(
                        "[{}] Connection error: {}, reconnecting in 5s...",
                        exchange_name,
                        e
                    );
                    metrics.record_reconnect();
                }
            }

            // Reset order books for this exchange on reconnect
            for symbol in connector.supported_symbols() {
                if let Some(_book) = orderbook_manager.get(exchange_name, &symbol) {
                    tracing::info!("[{}] Resetting order book for {}", exchange_name, symbol);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    /// Connect to exchange and process messages
    async fn connect_and_process(
        connector: ExchangeConnector,
        client_broadcast_tx: broadcast::Sender<ClientMessage>,
        orderbook_manager: SharedOrderBookManager,
        metrics: SharedMetrics,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let exchange_name = connector.exchange().name();
        let symbols_owned = connector.supported_symbols();
        let symbols: Vec<&str> = symbols_owned.iter().map(|s| s.as_str()).collect();

        // 1. Initialize orderbooks from REST API (if needed)
        Self::initialize_orderbooks_from_rest(
            &connector,
            &symbols,
            &orderbook_manager,
            exchange_name,
        )
        .await;

        // 2. Connect to exchange WebSocket
        let (mut exchange_ws_write, mut exchange_ws_read) =
            Self::connect_websocket(&connector, &symbols).await?;

        // 3. Subscribe to streams (if needed)
        Self::subscribe_to_streams(&connector, &symbols, &mut exchange_ws_write, exchange_name)
            .await?;

        // 4. Process messages from exchange
        Self::process_websocket_messages(
            &mut exchange_ws_read,
            &connector,
            client_broadcast_tx,
            orderbook_manager,
            metrics,
            exchange_name,
        )
        .await?;

        Ok(())
    }

    /// Initialize orderbooks from REST API snapshots (skip for exchanges that send initial snapshot via WebSocket)
    async fn initialize_orderbooks_from_rest(
        connector: &ExchangeConnector,
        symbols: &[&str],
        orderbook_manager: &SharedOrderBookManager,
        exchange_name: &str,
    ) {
        let exchange = connector.exchange();
        let skip_rest_snapshot = matches!(exchange, super::Exchange::Bybit);

        if skip_rest_snapshot {
            tracing::info!(
                "[{}] Skipping REST snapshot fetch (WebSocket sends initial snapshot)",
                exchange_name
            );
            return;
        }

        tracing::info!("[{}] Fetching initial snapshots...", exchange_name);
        let mut initialized_count = 0;

        for symbol in symbols {
            tracing::info!(
                "[{}] Fetching initial order book snapshot for {}...",
                exchange_name,
                symbol
            );

            match connector.fetch_snapshot(symbol, 10).await {
                Ok(snapshot) => {
                    let mut book = orderbook_manager.get_or_create(exchange_name, symbol);
                    book.initialize_from_snapshot(
                        snapshot.bids,
                        snapshot.asks,
                        snapshot.last_update_id,
                    );
                    initialized_count += 1;
                }
                Err(e) => {
                    tracing::error!(
                        "[{}] Failed to fetch snapshot for {}: {}",
                        exchange_name,
                        symbol,
                        e
                    );
                }
            }
        }

        tracing::info!(
            "[{}] Initialized {}/{} order books",
            exchange_name,
            initialized_count,
            symbols.len()
        );
    }

    /// Connect to exchange WebSocket
    async fn connect_websocket(
        connector: &ExchangeConnector,
        symbols: &[&str],
    ) -> Result<
        (
            futures_util::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                WsMessage,
            >,
            futures_util::stream::SplitStream<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
            >,
        ),
        Box<dyn Error + Send + Sync>,
    > {
        let exchange_name = connector.exchange().name();
        let url = connector.build_subscription_url(symbols);

        tracing::info!("[{}] Connecting to WebSocket: {}...", exchange_name, url);
        let (ws_stream, _) = connect_async(&url).await?;
        tracing::info!("[{}] WebSocket connected", exchange_name);

        Ok(ws_stream.split())
    }

    /// Subscribe to streams (for exchanges that require post-connection subscription)
    async fn subscribe_to_streams(
        connector: &ExchangeConnector,
        symbols: &[&str],
        exchange_ws_write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            WsMessage,
        >,
        exchange_name: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(sub_msg) = connector.get_subscription_message(symbols) {
            tracing::info!("[{}] Sending subscription message...", exchange_name);
            exchange_ws_write
                .send(WsMessage::Text(sub_msg.into()))
                .await?;
            tracing::info!("[{}] Subscription sent", exchange_name);
        }
        Ok(())
    }

    /// Process WebSocket messages in a loop
    async fn process_websocket_messages(
        exchange_ws_read: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        connector: &ExchangeConnector,
        client_broadcast_tx: broadcast::Sender<ClientMessage>,
        orderbook_manager: SharedOrderBookManager,
        metrics: SharedMetrics,
        exchange_name: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        while let Some(exchange_ws_msg) = exchange_ws_read.next().await {
            let exchange_ws_msg = exchange_ws_msg?;

            match exchange_ws_msg {
                WsMessage::Text(text) => {
                    Self::handle_text_message(
                        &text,
                        connector,
                        &client_broadcast_tx,
                        &orderbook_manager,
                        &metrics,
                        exchange_name,
                    )
                    .await;
                }
                WsMessage::Binary(_) => {
                    // Some exchanges use binary messages
                }
                WsMessage::Ping(_) | WsMessage::Pong(_) => {
                    // Heartbeat - ignore
                }
                WsMessage::Close(_) => {
                    tracing::info!("[{}] WebSocket closed by server", exchange_name);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Handle a single text message from the WebSocket
    async fn handle_text_message(
        text: &str,
        connector: &ExchangeConnector,
        client_broadcast_tx: &broadcast::Sender<ClientMessage>,
        orderbook_manager: &SharedOrderBookManager,
        metrics: &SharedMetrics,
        exchange_name: &str,
    ) {
        let start = std::time::Instant::now();

        // Record raw metrics
        metrics.record_bytes(text.len() as u64);

        // Parse message via connector
        match connector.parse_message(text) {
            Ok(Some(market_msg)) => {
                // Record message with symbol
                let symbol = match &market_msg {
                    MarketMessage::DepthUpdate { symbol, .. } => symbol.clone(),
                    MarketMessage::Trade(trade) => trade.symbol.clone(),
                    MarketMessage::Raw(_) => String::new(),
                };
                Self::process_market_message(market_msg, client_broadcast_tx, orderbook_manager, metrics).await;

                metrics.record_latency(&symbol, start);
                if !symbol.is_empty() {
                    metrics.record_nb_message(&symbol);
                }
            }
            Ok(None) => {
                // Message parsed but not relevant (e.g., heartbeat)
            }
            Err(e) => {
                tracing::debug!("[{}] Failed to parse message: {}", exchange_name, e);
            }
        }
    }

    /// Process a normalized market message and broadcast to clients
    async fn process_market_message(
        msg: MarketMessage,
        client_broadcast_tx: &broadcast::Sender<ClientMessage>,
        orderbook_manager: &SharedOrderBookManager,
        metrics: &SharedMetrics,
    ) {
        match msg {
            MarketMessage::DepthUpdate {
                exchange,
                symbol,
                bids,
                asks,
                update_id,
                is_snapshot,
            } => {
                let exchange_name = exchange.name();

                let bids_str: Vec<(String, String)> = bids
                    .iter()
                    .map(|(p, q)| (p.to_string(), q.to_string()))
                    .collect();

                let asks_str: Vec<(String, String)> = asks
                    .iter()
                    .map(|(p, q)| (p.to_string(), q.to_string()))
                    .collect();

                let changed = {
                    let mut book = orderbook_manager.get_or_create(exchange_name, &symbol);

                    if is_snapshot {
                        book.initialize_from_snapshot(bids_str, asks_str, update_id);
                        book.trim(ORDERBOOK_DEPTH);
                        tracing::info!("[{}] Orderbook reset from snapshot: {}", exchange_name, symbol);
                        true
                    } else {
                        let changed = book.apply_update(bids_str, asks_str, 0, update_id);
                        if changed {
                            book.trim(ORDERBOOK_DEPTH);
                        }
                        changed
                    }
                };

                if changed {
                    metrics.record_nb_update();

                    if let Some(book_ref) = orderbook_manager.get(exchange_name, &symbol) {
                        let client_msg = book_ref.value().to_client_message(ORDERBOOK_DISPLAY_DEPTH);
                        let _ = client_broadcast_tx.send(client_msg);
                    }
                }
            }
            MarketMessage::Trade(trade) => {
                metrics.record_trade_for_symbol(&trade.symbol);
                let _ = client_broadcast_tx.send(ClientMessage::Trade(trade));
            }
            MarketMessage::Raw(_) => {
                // Debug messages - ignore
            }
        }
    }
}
