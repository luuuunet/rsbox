#!/usr/bin/env bash
# Performance benchmark script for rsbox

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RSBOX_BINARY="${RSBOX_BINARY:-$SCRIPT_DIR/../target/release/rsbox}"
CONFIG_FILE="${CONFIG_FILE:-$SCRIPT_DIR/../examples/config-benchmark.json}"
DURATION="${DURATION:-60}"
CONNECTIONS="${CONNECTIONS:-100}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

check_dependencies() {
    local missing=()

    for cmd in curl ab wrk; do
        if ! command -v $cmd >/dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done

    if [ ${#missing[@]} -gt 0 ]; then
        echo -e "${YELLOW}Warning: Missing tools: ${missing[*]}${NC}"
        echo "Install with: sudo apt-get install apache2-utils curl"
    fi
}

start_rsbox() {
    echo -e "${GREEN}Starting rsbox...${NC}"
    $RSBOX_BINARY run -c $CONFIG_FILE &
    RSBOX_PID=$!
    sleep 3

    if ! kill -0 $RSBOX_PID 2>/dev/null; then
        echo -e "${RED}Failed to start rsbox${NC}"
        exit 1
    fi
}

stop_rsbox() {
    if [ ! -z "$RSBOX_PID" ]; then
        echo -e "${YELLOW}Stopping rsbox...${NC}"
        kill $RSBOX_PID 2>/dev/null || true
        wait $RSBOX_PID 2>/dev/null || true
    fi
}

benchmark_http() {
    echo ""
    echo -e "${GREEN}=== HTTP Throughput Test ===${NC}"

    if command -v wrk >/dev/null 2>&1; then
        wrk -t4 -c$CONNECTIONS -d${DURATION}s --latency \
            -H "Connection: keep-alive" \
            --timeout 10s \
            http://httpbin.org/get
    else
        echo -e "${YELLOW}wrk not found, skipping${NC}"
    fi
}

benchmark_connections() {
    echo ""
    echo -e "${GREEN}=== Connection Test ===${NC}"

    if command -v ab >/dev/null 2>&1; then
        ab -n 10000 -c $CONNECTIONS -k \
            http://httpbin.org/get
    else
        echo -e "${YELLOW}ab not found, skipping${NC}"
    fi
}

measure_memory() {
    echo ""
    echo -e "${GREEN}=== Memory Usage ===${NC}"

    if [ ! -z "$RSBOX_PID" ]; then
        ps -p $RSBOX_PID -o pid,rss,vsz,cmd
    fi
}

cleanup() {
    stop_rsbox
}

trap cleanup EXIT INT TERM

main() {
    echo -e "${GREEN}rsbox Performance Benchmark${NC}"
    echo "Duration: ${DURATION}s"
    echo "Connections: $CONNECTIONS"
    echo ""

    check_dependencies
    start_rsbox

    sleep 2

    measure_memory
    benchmark_http
    benchmark_connections
    measure_memory

    echo ""
    echo -e "${GREEN}Benchmark complete!${NC}"
}

main "$@"
