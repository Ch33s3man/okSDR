#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
cd "$DIR"

echo "=========================================================="
echo " Starting okSDR - for Xiegu G90 by KE2JBD and Antigravity"
echo " Audio Input: Cubilux CB5 (96 kHz Stereo I/Q)"
echo " CAT Control: Hamlib rigctld (managed by okSDR)"
echo "=========================================================="

# IMPORTANT (Xiegu G90 PTT safety):
#   This script must NOT touch the serial port. The old version ran
#   `stty -F <dev> 19200 -hupcl`, which opened the port (Linux raises DTR+RTS on open)
#   and closed it with HUPCL disabled, leaving DTR/RTS HIGH and the G90 keyed in TX.
#   okSDR now opens the port itself with DTR/RTS forced LOW, and only after you press
#   Connect on the Radio Connection screen.

# Clean up a persistent rigctld left over from the old launcher (it listened on 4532).
if pgrep -f "rigctld.* -t 4532" >/dev/null 2>&1; then
    echo "[Info] Stopping leftover rigctld on port 4532 (okSDR now manages rigctld itself)..."
    pkill -f "rigctld.* -t 4532" 2>/dev/null || true
    sleep 0.5
fi

if ! command -v rigctld >/dev/null 2>&1; then
    echo "[Warning] Hamlib 'rigctld' is not installed. CAT control will be unavailable."
    echo "[Warning] Install it with:  sudo apt install libhamlib-utils"
fi

# Run SDR application (prefer release, fallback to debug)
if [ -f "./target/release/oksdr" ]; then
    echo "[Info] Launching optimized release binary..."
    exec ./target/release/oksdr "$@"
elif [ -f "./target/debug/oksdr" ]; then
    echo "[Info] Launching debug binary..."
    exec ./target/debug/oksdr "$@"
else
    echo "[Info] Building and launching..."
    exec cargo run --release -- "$@"
fi
