#!/bin/bash

# Script pour exécuter les tests de charge de FlowRS
# Usage: ./run_load_test.sh

set -e

echo "🚀 FlowRS Load Test Runner"
echo "═══════════════════════════════════════"
echo ""

# Vérifier que le serveur tourne
echo "📡 Checking if server is running on localhost:8080..."
if ! nc -z localhost 8080 2>/dev/null; then
    echo "❌ Server is not running!"
    echo ""
    echo "Please start the server first:"
    echo "  cargo run --release"
    echo ""
    exit 1
fi

echo "✅ Server detected"
echo ""

# Compiler en mode release
echo "🔨 Building load test in release mode..."
cargo build --release --tests
echo ""

# Exécuter les tests
echo "🧪 Running load tests..."
echo ""

# Option 1: Exécuter le main du load_test
cargo run --release --bin load_test 2>&1 | tee load_test_results.txt

# Ou Option 2: Exécuter les tests individuels
# cargo test --release --test load_test -- --ignored --nocapture

echo ""
echo "✅ Load tests completed!"
echo ""
echo "Results saved to: load_test_results.txt"
echo ""
