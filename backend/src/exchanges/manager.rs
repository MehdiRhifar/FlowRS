/// Manages WebSocket connections to multiple exchanges with auto-reconnect
use super::{ExchangeConnector, MarketMessage};
use crate::metrics::SharedMetrics;
use crate::orderbook::SharedOrderBookManager;
use crate::types::ClientMessage;
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

        tracing::info!("Starting {} exchange connection(s)", self.connectors.len());

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

    /// Initialize orderbooks from REST API snapshots (if needed)
    ///
    /// Exchanges that use WebSocket snapshots (Kraken, Coinbase, Bybit) return Ok(None).
    /// Only Binance currently fetches REST snapshots.
    async fn initialize_orderbooks_from_rest(
        connector: &ExchangeConnector,
        symbols: &[&str],
        orderbook_manager: &SharedOrderBookManager,
        exchange_name: &str,
    ) {
        let mut initialized_count = 0;

        for symbol in symbols {
            match connector.fetch_snapshot(symbol, 10).await {
                Ok(Some(snapshot)) => {
                    tracing::debug!("[{}] REST snapshot for {}", exchange_name, symbol);
                    let mut book = orderbook_manager.get_or_create(exchange_name, symbol);
                    book.initialize_from_snapshot(
                        snapshot.bids,
                        snapshot.asks,
                        snapshot.last_update_id,
                    );
                    initialized_count += 1;
                }
                Ok(None) => {
                    // Exchange uses WebSocket snapshots - skip REST fetch
                    tracing::debug!("[{}] {} uses WebSocket snapshots", exchange_name, symbol);
                }
                Err(e) => {
                    tracing::warn!(
                        "[{}] Snapshot fetch failed for {}: {}",
                        exchange_name,
                        symbol,
                        e
                    );
                }
            }
        }

        if initialized_count > 0 {
            tracing::info!(
                "[{}] Initialized {}/{} order books from REST",
                exchange_name,
                initialized_count,
                symbols.len()
            );
        }
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
        let sub_messages = connector.get_subscription_messages(symbols);
        if !sub_messages.is_empty() {
            tracing::info!(
                "[{}] Sending {} subscription message(s)...",
                exchange_name,
                sub_messages.len()
            );

            for (i, sub_msg) in sub_messages.iter().enumerate() {
                tracing::debug!(
                    "[{}] Sending subscription #{}: {}",
                    exchange_name,
                    i + 1,
                    sub_msg
                );
                exchange_ws_write
                    .send(WsMessage::Text(sub_msg.clone().into()))
                    .await?;
            }

            tracing::info!("[{}] All subscriptions sent", exchange_name);
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
                // Check if it's a relevant message (not Raw) before processing
                let is_relevant = !matches!(&market_msg, MarketMessage::Raw(_));

                Self::process_market_message(market_msg, client_broadcast_tx, orderbook_manager)
                    .await;

                metrics.record_latency(start);
                if is_relevant {
                    metrics.record_message();
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

                let mut book = orderbook_manager.get_or_create(exchange_name, &symbol);

                if is_snapshot {
                    book.initialize_from_snapshot(bids, asks, update_id);
                    tracing::debug!("[{}] Snapshot received for {}", exchange_name, symbol);
                    // No broadcast - server will poll orderbook state
                } else {
                    book.apply_update(bids, asks, 0, update_id);
                    // No broadcast - server will poll orderbook state
                }
            }
            MarketMessage::Trade(trade) => {
                let _ = client_broadcast_tx.send(ClientMessage::Trade(trade));
            }
            MarketMessage::Raw(_) => {
                // Debug messages - ignore
            }
        }
    }
}
