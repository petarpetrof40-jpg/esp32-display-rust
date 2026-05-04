# esp32-display-rust

Rust firmware for the **ESP32-2432S024** development board featuring:

- **MCU**: ESP32-WROOM-32D (Xtensa LX6, 240 MHz)
- **Display**: ST7789 240 × 320 colour TFT (SPI)
- **Touch**: XPT2046 resistive touchscreen controller (shared SPI bus)

## What it does

1. Initialises the ST7789 display via SPI.
2. Draws a white background with:
   - `"Hello, world!"` centred at (120, 30).
   - `"Touch screen to test"` centred on the screen.
3. Reads the XPT2046 touch controller (shared SPI bus, GPIO 36 IRQ).
4. When a touch is detected:
   - Displays X, Y and Pressure values in the lower portion of the screen.
   - Prints the same values to the serial monitor at **115200 baud**.

## GPIO pin mapping

| Signal     | GPIO |
|------------|------|
| TFT_MISO   |  12  |
| TFT_MOSI   |  13  |
| TFT_SCLK   |  14  |
| TFT_CS     |  15  |
| TFT_DC     |   2  |
| TFT_BL     |  27  |
| T_CS       |  33  |
| T_IRQ      |  36  |

`T_MOSI`, `T_MISO` and `T_CLK` are shared with the display SPI bus.

## Prerequisites

### Rust toolchain

The project uses the **`esp`** Rust channel which adds Xtensa support.
Install it with [esp-rs/rust-build](https://github.com/esp-rs/rust-build):

```sh
# Install the esp-rs Rust toolchain (once per machine)
cargo install espup
espup install
```

Or use the pre-built installer from <https://github.com/esp-rs/rust-build/releases>.

### ldproxy linker wrapper

```sh
cargo install ldproxy
```

### espflash (optional, for flashing)

```sh
cargo install espflash
```

### ESP-IDF

`embuild` (called from `build.rs`) automatically downloads and builds
ESP-IDF v5.2.3 on first compile.  No manual IDF installation is required.

## Build

```sh
cargo build --release
```

The first build downloads ESP-IDF and may take several minutes.

## Flash & monitor

```sh
# Replace /dev/ttyUSB0 with your device port (COMx on Windows)
espflash flash --monitor target/xtensa-esp32-espidf/release/esp32-display-rust
```

Or simply run:

```sh
cargo run --release
```

(The `runner` in `.cargo/config.toml` invokes `espflash flash --monitor`.)

## Project structure

```
.
├── .cargo/config.toml   # Cargo target, linker, ESP_IDF_VERSION
├── build.rs             # embuild – generates ESP-IDF sysenv bindings
├── Cargo.toml           # Dependencies and features
├── rust-toolchain.toml  # Pins to the esp Rust channel
├── sdkconfig.defaults   # ESP-IDF config (baud rate, stack sizes, …)
└── src/
    └── main.rs          # Application code
```

## Crates used

| Crate                  | Purpose                                   |
|------------------------|-------------------------------------------|
| `esp-idf-svc`          | ESP-IDF services, startup, logging        |
| `esp-idf-hal`          | SPI, GPIO, delay HAL drivers              |
| `embedded-hal`         | Hardware-abstraction traits (v1.0)        |
| `display-interface-spi`| Bridges `SpiDevice` → `display-interface` |
| `mipidsi`              | ST7789 display driver                     |
| `embedded-graphics`    | Text and primitive drawing                |
| `anyhow`               | Ergonomic error propagation               |