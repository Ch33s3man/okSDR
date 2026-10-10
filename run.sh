#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
cd "$DIR"

echo "=========================================================="
echo " Starting okSDR - for Xiegu G90 by KE2JBD and Antigravity"
echo " Audio Input: Cubilux CB5 (96 kHz Stereo I/Q)"
echo " CAT Control: Hamlib rigctld (Model 3089, Native Xiegu CI-V)"
echo "=========================================================="

# Optional: --restart-cat to force kill and restart rigctld
if [ "$1" = "--restart-cat" ]; then
    echo "[Info] Explicit CAT restart requested. Terminating rigctld..."
    killall -9 rigctld 2>/dev/null || true
    sleep 0.5
fi

# Find serial device for Xiegu G90 CAT (Xiegu DE-19 / CH340 / CH341)
SERIAL_DEV=""

# 1. Search /dev/serial/by-id for USB serial devices
if [ -d "/dev/serial/by-id" ]; then
    for dev in /dev/serial/by-id/*; do
        if [ -e "$dev" ]; then
            SERIAL_DEV=$(readlink -f "$dev")
            echo "[Info] Detected serial device by ID: $dev -> $SERIAL_DEV"
            break
        fi
    done
fi

# 2. Search /dev/ttyUSB* (0 through 9)
if [ -z "$SERIAL_DEV" ]; then
    for dev in /dev/ttyUSB*; do
        if [ -e "$dev" ]; then
            SERIAL_DEV="$dev"
            echo "[Info] Detected serial device: $SERIAL_DEV"
            break
        fi
    done
fi

# 3. Search /dev/ttyACM* (0 through 9)
if [ -z "$SERIAL_DEV" ]; then
    for dev in /dev/ttyACM*; do
        if [ -e "$dev" ]; then
            SERIAL_DEV="$dev"
            echo "[Info] Detected ACM serial device: $SERIAL_DEV"
            break
        fi
    done
fi

# Check existing rigctld daemon health
if pgrep -x rigctld >/dev/null 2>&1; then
    # Test if existing rigctld is communicating with radio
    RESP=$(echo "f" | timeout 0.6 nc 127.0.0.1 4532 2>/dev/null || true)
    if [[ "$RESP" =~ ^[0-9]{5,}$ ]]; then
        echo "[Info] Existing rigctld daemon active and communicating with G90 (${RESP} Hz)."
    else
        echo "[Info] Existing rigctld is not responding to G90 commands (radio unplugged or port changed). Resetting..."
        killall -9 rigctld 2>/dev/null || true
        sleep 0.5
    fi
fi

# Launch rigctld if needed and serial device is found
if [ -n "$SERIAL_DEV" ]; then
    if ! pgrep -x rigctld >/dev/null 2>&1; then
        echo "[Info] Starting persistent rigctld on $SERIAL_DEV..."
        stty -F "$SERIAL_DEV" 19200 -hupcl -crtscts 2>/dev/null || true
        nohup rigctld -m 3089 -r "$SERIAL_DEV" -s 19200 \
            --set-conf=civaddr=0x70,dtr_state=OFF,rts_state=OFF,serial_handshake=None,ptt_type=None,cache_timeout=0 \
            -t 4532 >/dev/null 2>&1 &
        sleep 0.8
    fi
else
    echo "[Warning] No USB serial device (/dev/ttyUSB*, /dev/ttyACM*, /dev/serial/by-id/*) detected."
    echo "[Warning] Please verify that your Xiegu DE-19 USB cable is plugged in and the G90 is powered on."
fi

# Run SDR application (prefer release, fallback to debug)
if [ -f "./target/release/oksdr" ]; then
    echo "[Info] Launching optimized release binary..."
    ./target/release/oksdr
elif [ -f "./target/debug/oksdr" ]; then
    echo "[Info] Launching debug binary..."
    ./target/debug/oksdr
elif [ -f "./target/release/custom_sdr" ]; then
    echo "[Info] Launching legacy release binary..."
    ./target/release/custom_sdr
else
    echo "[Info] Building and launching..."
    cargo run --release
fi
