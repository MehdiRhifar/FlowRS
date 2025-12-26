use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::StreamExt;

/// Configuration pour les tests de charge
struct LoadTestConfig {
    num_clients: usize,
    duration_secs: u64,
    server_url: String,
}

/// Résultats du test de charge
#[derive(Debug)]
struct LoadTestResults {
    clients: usize,
    duration: Duration,
    messages_received: u64,
    messages_per_second: f64,
    errors: u64,
    disconnections: u64,
    latencies: Vec<Duration>,
}

impl LoadTestResults {
    fn report(&self) -> String {
        let p50 = self.percentile(0.50);
        let p95 = self.percentile(0.95);
        let p99 = self.percentile(0.99);
        let max = self.latencies.iter().max().copied().unwrap_or_default();

        format!(
            r#"
📊 LOAD TEST RESULTS
═══════════════════════════════════════

Configuration:
  • Clients:        {}
  • Duration:       {:.1}s
  • Target:         {}

Performance:
  • Messages:       {} total
  • Throughput:     {:.0} msg/sec
  • Errors:         {}
  • Disconnects:    {}

Latency Distribution:
  • P50:            {:?}
  • P95:            {:?}
  • P99:            {:?}
  • Max:            {:?}

Status: {}
"#,
            self.clients,
            self.duration.as_secs_f64(),
            "ws://localhost:8080",
            self.messages_received,
            self.messages_per_second,
            self.errors,
            self.disconnections,
            p50,
            p95,
            p99,
            max,
            if self.errors == 0 && self.disconnections == 0 {
                "✅ PASS"
            } else {
                "⚠️  DEGRADED"
            }
        )
    }

    fn percentile(&self, p: f64) -> Duration {
        if self.latencies.is_empty() {
            return Duration::from_secs(0);
        }
        let mut sorted = self.latencies.clone();
        sorted.sort();
        let idx = ((sorted.len() as f64 * p) as usize).min(sorted.len() - 1);
        sorted[idx]
    }
}

/// Client WebSocket pour les tests de charge
async fn load_test_client(
    client_id: usize,
    config: Arc<LoadTestConfig>,
    messages: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    disconnections: Arc<AtomicU64>,
) {
    let start = Instant::now();
    let duration = Duration::from_secs(config.duration_secs);

    loop {
        if start.elapsed() >= duration {
            break;
        }

        // Connexion au serveur
        let connect_result = connect_async(&config.server_url).await;
        let (ws_stream, _) = match connect_result {
            Ok(result) => result,
            Err(e) => {
                eprintln!("Client {} connection failed: {}", client_id, e);
                errors.fetch_add(1, Ordering::Relaxed);
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        println!("Client {} connected", client_id);

        let (_write, mut read) = ws_stream.split();

        // Lire les messages jusqu'à déconnexion ou timeout
        while start.elapsed() < duration {
            match tokio::time::timeout(Duration::from_secs(5), read.next()).await {
                Ok(Some(Ok(Message::Text(_)))) => {
                    messages.fetch_add(1, Ordering::Relaxed);
                }
                Ok(Some(Ok(Message::Binary(_)))) => {
                    messages.fetch_add(1, Ordering::Relaxed);
                }
                Ok(Some(Ok(Message::Close(_)))) => {
                    println!("Client {} received close", client_id);
                    disconnections.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                Ok(Some(Err(e))) => {
                    eprintln!("Client {} error: {}", client_id, e);
                    errors.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                Ok(None) => {
                    println!("Client {} stream ended", client_id);
                    disconnections.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                Err(_) => {
                    // Timeout - pas grave, on continue
                    continue;
                }
                _ => {}
            }
        }

        // Si le test n'est pas fini, on reconnecte
        if start.elapsed() < duration {
            println!("Client {} reconnecting...", client_id);
            sleep(Duration::from_millis(100)).await;
        }
    }

    println!("Client {} finished", client_id);
}

/// Exécute un test de charge
async fn run_load_test(config: LoadTestConfig) -> LoadTestResults {
    let config = Arc::new(config);
    let messages = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let disconnections = Arc::new(AtomicU64::new(0));

    println!(
        "\n🚀 Starting load test with {} clients for {}s...\n",
        config.num_clients, config.duration_secs
    );

    let start = Instant::now();

    // Lancer tous les clients
    let mut handles = vec![];
    for i in 0..config.num_clients {
        let config = Arc::clone(&config);
        let messages = Arc::clone(&messages);
        let errors = Arc::clone(&errors);
        let disconnections = Arc::clone(&disconnections);

        let handle = tokio::spawn(async move {
            load_test_client(i, config, messages, errors, disconnections).await;
        });

        handles.push(handle);

        // Étaler le démarrage pour éviter de surcharger au démarrage
        if i % 10 == 0 {
            sleep(Duration::from_millis(100)).await;
        }
    }

    // Attendre que tous les clients terminent
    for handle in handles {
        let _ = handle.await;
    }

    let duration = start.elapsed();
    let total_messages = messages.load(Ordering::Relaxed);
    let total_errors = errors.load(Ordering::Relaxed);
    let total_disconnections = disconnections.load(Ordering::Relaxed);

    LoadTestResults {
        clients: config.num_clients,
        duration,
        messages_received: total_messages,
        messages_per_second: total_messages as f64 / duration.as_secs_f64(),
        errors: total_errors,
        disconnections: total_disconnections,
        latencies: vec![], // Pour l'instant, on ne mesure pas les latencies individuelles
    }
}

#[tokio::test]
#[ignore] // Ignorer par défaut car nécessite un serveur qui tourne
async fn test_load_single_client() {
    let config = LoadTestConfig {
        num_clients: 1,
        duration_secs: 10,
        server_url: "ws://localhost:8080".to_string(),
    };

    let results = run_load_test(config).await;
    println!("{}", results.report());

    // Assertions
    assert!(results.messages_received > 0, "Should receive messages");
    assert_eq!(results.errors, 0, "Should have no errors");
}

#[tokio::test]
#[ignore]
async fn test_load_multiple_clients() {
    let config = LoadTestConfig {
        num_clients: 10,
        duration_secs: 30,
        server_url: "ws://localhost:8080".to_string(),
    };

    let results = run_load_test(config).await;
    println!("{}", results.report());

    assert!(results.messages_received > 100, "Should receive many messages");
    assert!(
        results.messages_per_second > 10.0,
        "Should maintain good throughput"
    );
}

#[tokio::test]
#[ignore]
async fn test_load_stress() {
    let config = LoadTestConfig {
        num_clients: 50,
        duration_secs: 60,
        server_url: "ws://localhost:8080".to_string(),
    };

    let results = run_load_test(config).await;
    println!("{}", results.report());

    // Le système devrait tenir la charge
    assert!(results.messages_received > 1000);
    assert!(results.messages_per_second > 50.0);
}

/// Test principal pour exécution manuelle
#[tokio::main]
async fn main() {
    // Test progressif
    let tests = vec![
        ("Warmup", 1, 5),
        ("Light", 5, 10),
        ("Medium", 10, 30),
        ("Heavy", 25, 30),
        ("Stress", 50, 60),
    ];

    println!("\n🧪 FLOWRS LOAD TEST SUITE");
    println!("═══════════════════════════════════════\n");

    let mut all_results = vec![];

    for (name, clients, duration) in tests {
        println!("\n📋 Test: {} ({} clients, {}s)", name, clients, duration);
        println!("───────────────────────────────────────\n");

        let config = LoadTestConfig {
            num_clients: clients,
            duration_secs: duration,
            server_url: "ws://localhost:8080".to_string(),
        };

        let results = run_load_test(config).await;
        println!("{}", results.report());

        all_results.push((name, results));

        // Pause entre les tests
        if duration < 60 {
            println!("Cooling down for 5 seconds...\n");
            sleep(Duration::from_secs(5)).await;
        }
    }

    // Rapport final
    println!("\n\n📈 FINAL SUMMARY");
    println!("═══════════════════════════════════════\n");
    println!("{:<10} {:<10} {:<15} {:<10}", "Test", "Clients", "Messages", "Msg/sec");
    println!("───────────────────────────────────────");

    for (name, result) in &all_results {
        println!(
            "{:<10} {:<10} {:<15} {:<10.0}",
            name, result.clients, result.messages_received, result.messages_per_second
        );
    }

    println!("\n✅ All tests completed!\n");
}
