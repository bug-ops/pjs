#!/usr/bin/env bash
# Performance profiling script for PJS
#
# Usage:
#   ./scripts/profile.sh cpu       # CPU flamegraph profiling
#   ./scripts/profile.sh heap      # Heap allocation profiling with dhat
#   ./scripts/profile.sh bench     # Run benchmarks with profiling enabled

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_tool() {
    if ! command -v "$1" &> /dev/null; then
        log_error "$1 not found. Install with: cargo install $2"
        exit 1
    fi
}

profile_cpu() {
    log_info "Running CPU flamegraph profiling..."
    check_tool "cargo-flamegraph" "flamegraph"

    cd "$PROJECT_ROOT"

    log_info "Profiling simple_throughput benchmark..."
    cargo flamegraph --bench simple_throughput --profile bench \
        --output target/flamegraph-simple.svg

    log_info "Flamegraph saved to: target/flamegraph-simple.svg"
    log_info "Opening flamegraph in browser..."

    if command -v open &> /dev/null; then
        open target/flamegraph-simple.svg
    elif command -v xdg-open &> /dev/null; then
        xdg-open target/flamegraph-simple.svg
    else
        log_warn "Could not auto-open browser. View: target/flamegraph-simple.svg"
    fi
}

profile_heap() {
    log_info "Running heap allocation profiling with dhat..."

    cd "$PROJECT_ROOT"

    log_info "Building benchmarks with dhat-heap feature..."
    cargo build --bench simple_throughput --features dhat-heap --profile bench

    log_info "Running benchmark with heap profiling..."
    DHAT_PROFILER=1 cargo bench --bench simple_throughput --features dhat-heap -- --profile-time=10

    if [ -f "dhat-heap.json" ]; then
        log_info "Heap profile saved to: dhat-heap.json"
        log_info "View with: https://nnethercote.github.io/dh_view/dh_view.html"
    else
        log_warn "dhat-heap.json not found. Check if dhat integration is correct."
    fi
}

profile_bench() {
    log_info "Running benchmarks with profiling enabled..."

    cd "$PROJECT_ROOT"

    log_info "Running all benchmarks..."
    cargo bench -p pjs-bench -- --profile-time=10

    log_info "Benchmark results saved to: target/criterion/"
    log_info "Opening criterion report..."

    if [ -f "target/criterion/report/index.html" ]; then
        if command -v open &> /dev/null; then
            open target/criterion/report/index.html
        elif command -v xdg-open &> /dev/null; then
            xdg-open target/criterion/report/index.html
        else
            log_warn "Could not auto-open browser. View: target/criterion/report/index.html"
        fi
    else
        log_warn "Criterion report not found."
    fi
}

show_usage() {
    cat <<EOF
Performance Profiling Script for PJS

Usage:
    $0 <command>

Commands:
    cpu     Run CPU flamegraph profiling (requires cargo-flamegraph)
    heap    Run heap allocation profiling with dhat
    bench   Run criterion benchmarks with profiling
    help    Show this help message

Examples:
    $0 cpu              # Generate flamegraph for CPU profiling
    $0 heap             # Profile heap allocations
    $0 bench            # Run all benchmarks

Prerequisites:
    - cargo-flamegraph: cargo install flamegraph
    - perf (Linux): sudo apt-get install linux-perf
    - dtrace (macOS): comes with Xcode tools

Documentation:
    - See .local/performance-profile.md for detailed results
    - Flamegraphs: target/flamegraph-*.svg
    - Criterion reports: target/criterion/report/index.html
    - dhat reports: dhat-heap.json (view at https://nnethercote.github.io/dh_view/dh_view.html)
EOF
}

main() {
    case "${1:-help}" in
        cpu)
            profile_cpu
            ;;
        heap)
            profile_heap
            ;;
        bench)
            profile_bench
            ;;
        help|--help|-h)
            show_usage
            ;;
        *)
            log_error "Unknown command: $1"
            show_usage
            exit 1
            ;;
    esac
}

main "$@"
