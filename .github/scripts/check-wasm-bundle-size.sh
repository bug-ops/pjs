#!/bin/bash

# WASM Bundle Size Checker
# Analyzes WASM bundle sizes and compares against thresholds
# Usage: ./check-wasm-bundle-size.sh <path-to-wasm-binary>

set -e

WASM_FILE="${1:-crates/pjs-wasm/pkg/pjs_wasm_bg.wasm}"
MAX_RAW_KB=${2:-200}
MAX_GZIPPED_KB=${3:-80}

if [ ! -f "$WASM_FILE" ]; then
    echo "Error: WASM file not found: $WASM_FILE"
    exit 1
fi

echo "=== WASM Bundle Size Analysis ==="
echo "File: $WASM_FILE"
echo ""

# Calculate raw size
RAW_BYTES=$(wc -c < "$WASM_FILE")
RAW_KB=$((RAW_BYTES / 1024))

# Calculate gzipped size
GZIPPED_BYTES=$(gzip -c < "$WASM_FILE" | wc -c)
GZIPPED_KB=$((GZIPPED_BYTES / 1024))

# Calculate compression ratio
RATIO=$(awk "BEGIN {printf \"%.1f\", ($GZIPPED_BYTES / $RAW_BYTES) * 100}")

echo "Size Analysis:"
echo "  Raw:       $RAW_KB KB ($RAW_BYTES bytes)"
echo "  Gzipped:   $GZIPPED_KB KB ($GZIPPED_BYTES bytes)"
echo "  Ratio:     $RATIO% (compressed/original)"
echo ""

# Check thresholds
echo "Threshold Analysis:"
echo "  Max raw:     $MAX_RAW_KB KB"
echo "  Max gzipped: $MAX_GZIPPED_KB KB"
echo ""

STATUS="PASS"
if [ $RAW_KB -gt $MAX_RAW_KB ]; then
    echo "⚠ WARNING: Raw size $RAW_KB KB exceeds threshold $MAX_RAW_KB KB"
    STATUS="WARN"
fi

if [ $GZIPPED_KB -gt $MAX_GZIPPED_KB ]; then
    echo "⚠ WARNING: Gzipped size $GZIPPED_KB KB exceeds threshold $MAX_GZIPPED_KB KB"
    STATUS="WARN"
fi

if [ "$STATUS" = "PASS" ]; then
    echo "✓ All thresholds passed"
fi

echo ""
echo "Status: $STATUS"

# Exit with appropriate code
if [ "$STATUS" = "WARN" ]; then
    exit 1
fi

exit 0
