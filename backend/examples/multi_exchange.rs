/// Example: Multi-Exchange Order Book Aggregation
///
/// This example demonstrates how to connect to multiple exchanges
/// simultaneously and aggregate their order books.

use flowRS_backend::exchanges::{
    BinanceConn, BybitConn, ExchangeConnector, MarketMessage,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🚀 FlowRS Multi-Exchange Demo");
    println!("═══════════════════════════════════════\n");

    // Define symbols to track
    let symbols = vec!["BTCUSDT", "ETHUSDT"];

    // Create connectors for different exchanges
    let connectors: Vec<ExchangeConnector> = vec![
        ExchangeConnector::Binance(BinanceConn::new(
            symbols.iter().map(|s| s.to_string()).collect(),
        )),
        ExchangeConnector::Bybit(BybitConn::new(
            symbols.iter().map(|s| s.to_string()).collect(),
        )),
    ];

    println!("📡 Configured exchanges:");
    for connector in &connectors {
        println!("  • {}", connector.exchange().name());
        println!(
            "    Symbols: {}",
            connector.supported_symbols().join(", ")
        );
    }
    println!();

    // Fetch initial snapshots from all exchanges
    println!("📥 Fetching initial snapshots...\n");

    for connector in &connectors {
        let exchange = connector.exchange();
        println!("{}:", exchange.name());

        for symbol in connector.supported_symbols() {
            match connector.fetch_snapshot(&symbol, 5).await {
                Ok(snapshot) => {
                    println!("  ✅ {} - {} bids, {} asks, update_id: {}",
                        snapshot.symbol,
                        snapshot.bids.len(),
                        snapshot.asks.len(),
                        snapshot.last_update_id
                    );

                    // Display top 3 levels
                    if !snapshot.bids.is_empty() {
                        println!("     Best bid: {} @ {}", snapshot.bids[0].1, snapshot.bids[0].0);
                    }
                    if !snapshot.asks.is_empty() {
                        println!("     Best ask: {} @ {}", snapshot.asks[0].1, snapshot.asks[0].0);
                    }
                }
                Err(e) => {
                    println!("  ❌ {} - Error: {}", symbol, e);
                }
            }
        }
        println!();
    }

    // Example: Parse messages from different exchanges
    println!("📊 Example message parsing:\n");

    // Binance message example
    let binance_msg = r#"{
        "stream": "btcusdt@depth@100ms",
        "data": {
            "s": "BTCUSDT",
            "U": 100,
            "u": 101,
            "b": [["50000.00", "1.5"]],
            "a": [["50001.00", "2.0"]]
        }
    }"#;

    let binance = ExchangeConnector::Binance(BinanceConn::new(vec!["BTCUSDT".to_string()]));
    match binance.parse_message(binance_msg) {
        Ok(Some(MarketMessage::DepthUpdate {
            exchange,
            symbol,
            bids,
            asks,
            ..
        })) => {
            println!("✅ Parsed Binance depth update:");
            println!("   Exchange: {:?}", exchange);
            println!("   Symbol: {}", symbol);
            println!("   Bids: {:?}", bids.len());
            println!("   Asks: {:?}", asks.len());
        }
        Ok(_) => println!("⚠️  Unexpected message type"),
        Err(e) => println!("❌ Parse error: {}", e),
    }

    println!("\n✨ Demo completed!");
    println!("\nTo run live WebSocket connections:");
    println!("  cargo run --release");

    Ok(())
}
