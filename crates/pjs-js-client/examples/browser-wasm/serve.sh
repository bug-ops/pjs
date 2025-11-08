#!/bin/bash
# Simple HTTP server for testing the browser demo

echo "🚀 Starting PJS WASM Browser Demo..."
echo ""
echo "Opening browser at http://localhost:8000"
echo "Press Ctrl+C to stop the server"
echo ""

# Try different server options
if command -v python3 &> /dev/null; then
    python3 -m http.server 8000
elif command -v python &> /dev/null; then
    python -m SimpleHTTPServer 8000
elif command -v npx &> /dev/null; then
    npx http-server -p 8000
else
    echo "❌ No HTTP server found. Please install one of:"
    echo "   - Python 3: python3 -m http.server"
    echo "   - Node.js: npm install -g http-server"
    exit 1
fi
