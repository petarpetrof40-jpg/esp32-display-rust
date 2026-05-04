# esp32-display-rust

Rust firmware for the **ESP32-2432S024** board featuring:
- **ST7789** 240×320 colour TFT display (SPI, mipidsi driver)
- **XPT2046** resistive touchscreen controller (SPI, custom driver)

## Hardware

| Signal      | GPIO |
|-------------|------|
| TFT_MISO    | 12   |
| TFT_MOSI    | 13   |
| TFT_SCLK    | 14   |
| TFT_CS      | 15   |
| TFT_DC      | 2    |
| TFT_RST     | –1 (tied to EN internally) |
| TFT_BL      | 27   |
| T_CS        | 33   |
| T_IRQ       | 36   |
| T_MOSI/MISO/CLK | shared with display |

## Behaviour

1. **Boot** – white background, "Hello, world!" near the top, "Touch screen to test" in the centre.
2. **Touch detected** – reads calibrated X/Y coordinates and pressure, clears the data area and redraws the three values, prints them to the serial monitor at 115200 baud.
3. **Loop** – 100 ms debounce delay after each touch event.

## Building

### Prerequisites

Install the Espressif Rust toolchain and tooling:

```bash
cargo install espup
espup install
# Follow the printed instructions to source the export script, e.g.:
. $HOME/export-esp.sh

cargo install cargo-espflash espflash
```

### Compile

```bash
cargo build --release
```

### Flash & monitor

```bash
espflash flash --monitor target/xtensa-esp32-espidf/release/esp32-display-rust
```

## Project structure

```
.
├── .cargo/config.toml     # Build target (xtensa-esp32-espidf) and runner
├── src/main.rs            # Application code
├── build.rs               # esp-idf build integration (embuild)
├── Cargo.toml             # Dependencies
├── rust-toolchain.toml    # Espressif Rust toolchain pin
└── sdkconfig.defaults     # ESP-IDF SDK settings (stack size, baud rate …)
```

## Crates used

| Crate | Purpose |
|-------|---------|
| `esp-idf-svc` | ESP-IDF runtime, `std`, logging |
| `esp-idf-hal` | GPIO, SPI peripherals |
| `mipidsi` | ST7789 display driver |
| `display-interface-spi` | SPI ↔ display-interface adapter |
| `embedded-graphics` | 2-D text and shape rendering |
| `embedded-hal` | Hardware-abstraction traits |
| `anyhow` | Ergonomic error handling |