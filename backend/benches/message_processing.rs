use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rust_decimal::Decimal;
use std::str::FromStr;

// Importer les VRAIS types depuis votre code
use flowRS_backend::types::{BinanceDepthStream, BinanceTradeStream};
use flowRS_backend::orderbook::OrderBook;

// Sample data matching real Binance WebSocket messages
const DEPTH_MESSAGE: &str = r#"{
  "stream": "btcusdt@depth@100ms",
  "data": {
    "e": "depthUpdate",
    "E": 1234567890,
    "s": "BTCUSDT",
    "U": 157,
    "u": 160,
    "b": [
      ["50000.10", "1.5"],
      ["50000.00", "2.3"],
      ["49999.90", "0.8"],
      ["49999.80", "1.2"],
      ["49999.70", "3.5"]
    ],
    "a": [
      ["50001.10", "1.2"],
      ["50001.20", "2.1"],
      ["50001.30", "0.9"],
      ["50001.40", "1.8"],
      ["50001.50", "2.7"]
    ]
  }
}"#;

const TRADE_MESSAGE: &str = r#"{
  "stream": "btcusdt@aggTrade",
  "data": {
    "e": "aggTrade",
    "E": 1234567890,
    "s": "BTCUSDT",
    "a": 12345,
    "p": "50000.50",
    "q": "1.5",
    "f": 100,
    "l": 105,
    "T": 1234567890,
    "m": true
  }
}"#;

// Types importés depuis votre code réel - pas de duplication !
// Les benchmarks testent maintenant le VRAI code

// Benchmark JSON deserialization
fn bench_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parsing");

    group.bench_function("depth_message", |b| {
        b.iter(|| {
            serde_json::from_str::<BinanceDepthStream>(black_box(DEPTH_MESSAGE))
                .unwrap()
        })
    });

    group.bench_function("depth_message_from_bytes", |b| {
        let bytes = DEPTH_MESSAGE.as_bytes();
        b.iter(|| {
            serde_json::from_slice::<BinanceDepthStream>(black_box(bytes))
                .unwrap()
        })
    });

    group.bench_function("trade_message", |b| {
        b.iter(|| {
            serde_json::from_str::<BinanceTradeStream>(black_box(TRADE_MESSAGE))
                .unwrap()
        })
    });

    group.finish();
}

// Benchmark message type detection
fn bench_message_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_detection");

    group.bench_function("byte_pattern_depth", |b| {
        let bytes = DEPTH_MESSAGE.as_bytes();
        b.iter(|| {
            black_box(bytes.windows(6).any(|w| w == b"@depth"))
        })
    });

    group.bench_function("string_contains", |b| {
        b.iter(|| {
            black_box(DEPTH_MESSAGE.contains("@depth"))
        })
    });

    group.finish();
}

// Benchmark Decimal parsing
fn bench_decimal_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("decimal_parsing");

    let prices = vec![
        "50000.10",
        "0.00001234",
        "12345678.90123456",
        "1.5",
    ];

    for price in &prices {
        group.bench_with_input(
            BenchmarkId::new("from_str_exact", price),
            price,
            |b, p| {
                b.iter(|| {
                    Decimal::from_str_exact(black_box(p)).unwrap()
                })
            },
        );
    }

    group.finish();
}

// Benchmark OrderBook operations
fn bench_orderbook_operations(c: &mut Criterion) {
    use std::collections::BTreeMap;

    let mut group = c.benchmark_group("orderbook_operations");

    // Benchmark inserting into BTreeMap
    group.bench_function("btreemap_insert_10", |b| {
        b.iter(|| {
            let mut map = BTreeMap::<Decimal, Decimal>::new();
            for i in 0..10 {
                let price = Decimal::from_str(&format!("50000.{}", i)).unwrap();
                let qty = Decimal::from_str("1.5").unwrap();
                map.insert(price, qty);
            }
            map
        })
    });

    group.bench_function("btreemap_insert_100", |b| {
        b.iter(|| {
            let mut map = BTreeMap::<Decimal, Decimal>::new();
            for i in 0..100 {
                let price = Decimal::from_str(&format!("50000.{:03}", i)).unwrap();
                let qty = Decimal::from_str("1.5").unwrap();
                map.insert(price, qty);
            }
            map
        })
    });

    // Benchmark updating existing entries
    group.bench_function("btreemap_update_existing", |b| {
        let mut map = BTreeMap::<Decimal, Decimal>::new();
        for i in 0..100 {
            let price = Decimal::from_str(&format!("50000.{:03}", i)).unwrap();
            let qty = Decimal::from_str("1.5").unwrap();
            map.insert(price, qty);
        }

        b.iter(|| {
            for i in 0..10 {
                let price = Decimal::from_str(&format!("50000.{:03}", i)).unwrap();
                let qty = Decimal::from_str("2.5").unwrap();
                map.insert(price, qty);
            }
        })
    });

    // Benchmark getting top N levels
    group.bench_function("btreemap_top_10_levels", |b| {
        let mut map = BTreeMap::<Decimal, Decimal>::new();
        for i in 0..100 {
            let price = Decimal::from_str(&format!("50000.{:03}", i)).unwrap();
            let qty = Decimal::from_str("1.5").unwrap();
            map.insert(price, qty);
        }

        b.iter(|| {
            map.iter().take(10).collect::<Vec<_>>()
        })
    });

    // Benchmark trimming (split_off)
    group.bench_function("btreemap_trim", |b| {
        b.iter_batched(
            || {
                let mut map = BTreeMap::<Decimal, Decimal>::new();
                for i in 0..100 {
                    let price = Decimal::from_str(&format!("50000.{:03}", i)).unwrap();
                    let qty = Decimal::from_str("1.5").unwrap();
                    map.insert(price, qty);
                }
                map
            },
            |mut map| {
                if let Some((&price, _)) = map.iter().nth(10) {
                    map.split_off(&price);
                }
                map
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// Benchmark complete message processing pipeline
fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");

    // Test 1: Pipeline complet avec vraie OrderBook
    group.bench_function("real_depth_update_pipeline", |b| {
        // Setup: créer un OrderBook initialisé
        let mut book = OrderBook::new("BTCUSDT", "binancebinance");
        book.initialize_from_snapshot(
            vec![("50000.00".to_string(), "1.0".to_string())],
            vec![("50001.00".to_string(), "1.0".to_string())],
            100,
        );

        b.iter(|| {
            // 1. Parse le message JSON (vraie désérialisation)
            let msg: BinanceDepthStream =
                serde_json::from_str(black_box(DEPTH_MESSAGE)).unwrap();

            // 2. Appliquer l'update avec la VRAIE fonction apply_update
            book.apply_update(
                msg.data.bids,
                msg.data.asks,
                msg.data.first_update_id,
                msg.data.final_update_id,
            );

            // 3. Trim avec la vraie fonction
            book.trim(10);
        })
    });

    // Test 2: Pipeline complet + génération du message client
    group.bench_function("real_pipeline_with_client_message", |b| {
        let mut book = OrderBook::new("BTCUSDT", "binance");
        book.initialize_from_snapshot(
            vec![("50000.00".to_string(), "1.0".to_string())],
            vec![("50001.00".to_string(), "1.0".to_string())],
            100,
        );

        b.iter(|| {
            let msg: BinanceDepthStream =
                serde_json::from_str(black_box(DEPTH_MESSAGE)).unwrap();

            let changed = book.apply_update(
                msg.data.bids,
                msg.data.asks,
                msg.data.first_update_id,
                msg.data.final_update_id,
            );

            if changed {
                book.trim(10);
                // Générer le message client (ce qui est envoyé au frontend)
                black_box(book.to_client_message(10));
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_json_parsing,
    bench_message_detection,
    bench_decimal_parsing,
    bench_orderbook_operations,
    bench_full_pipeline
);
criterion_main!(benches);
