# Cyrene Companion Firmware

Firmware for microcontroller boards that communicate with Cyrene over a defined protocol (R37).

## Supported Boards

| Board | Status | Protocol |
|-------|--------|----------|
| ESP32 | Supported | JSON-RPC 2.0 over serial |

## Protocol

The host↔firmware protocol uses JSON-RPC 2.0 over serial (115200 baud, newline-delimited).

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `ping` | — | Returns uptime and protocol version |
| `version` | — | Returns firmware version and chip info |
| `read_pin` | `{ "pin": <int> }` | Reads GPIO pin value (0 or 1) |
| `write_pin` | `{ "pin": <int>, "value": <0\|1> }` | Writes GPIO pin value |

### Version Negotiation

The firmware reports its `protocol_version` on every `ping` and `version` response. The host (`cyrene-hardware` crate) refuses to communicate with firmware whose protocol version is incompatible.

## Build & Flash (ESP32)

```bash
# Install ESP-IDF: https://docs.espressif.com/projects/esp-idf/en/latest/esp32/get-started/
cd firmware/esp32
idf.py build
idf.py flash
idf.py monitor
```

## Architecture

```
┌──────────────┐   JSON-RPC 2.0    ┌──────────────────┐
│  Cyrene Host │ ◄─── serial ────► │  MCU Firmware    │
│ (cyrene-hw)  │   115200 baud     │  (ESP32/Pico)    │
└──────────────┘                   └──────────────────┘
```

The firmware runs a simple event loop: read JSON lines from serial, dispatch to handlers, write JSON responses. No dynamic memory allocation after init (heap-allocated cJSON only for parsing/formatting).
