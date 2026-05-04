//! ESP32-2432S024 demo: ST7789 display + XPT2046 touchscreen
//!
//! Hardware wiring (matches the physical board):
//!
//! | Signal     | GPIO |
//! |------------|------|
//! | TFT_MISO   |  12  |
//! | TFT_MOSI   |  13  |
//! | TFT_SCLK   |  14  |
//! | TFT_CS     |  15  |
//! | TFT_DC     |   2  |
//! | TFT_BL     |  27  |
//! | T_CS       |  33  |
//! | T_IRQ      |  36  |
//! | T_MOSI/MISO/CLK — shared with TFT
//!
//! The SPI bus is shared between the display and the touch controller using
//! two separate `SpiDeviceDriver` instances (each with its own CS pin and
//! clock frequency) borrowing a single `SpiDriver`.

use anyhow::Result;
use display_interface_spi::SPIInterface;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::{Rgb565, RgbColor},
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
};
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::PinDriver,
    peripherals::Peripherals,
    spi::{config::Config as SpiConfig, SpiDeviceDriver, SpiDriver, SpiDriverConfig},
    units::FromValueType,
};
use log::info;
use mipidsi::{
    models::ST7789,
    options::{ColorInversion, Orientation},
    Builder,
};

// ── Screen dimensions ────────────────────────────────────────────────────────
const SCREEN_WIDTH: i32 = 240;
const SCREEN_HEIGHT: i32 = 320;

// ── XPT2046 single-byte commands (12-bit differential mode, ADC powered) ────
// Format: 1_AAA_0_0_PD   (start=1, channel select AAA, 12-bit=0, diff=0, PD=01)
const XPT2046_CMD_X: u8 = 0xD1; // Channel 101 → X position
const XPT2046_CMD_Y: u8 = 0x91; // Channel 001 → Y position
const XPT2046_CMD_Z1: u8 = 0xB1; // Channel 011 → Z1 pressure

// ── Touchscreen calibration (raw ADC values from the Arduino sketch) ─────────
const TOUCH_X_MIN: i32 = 240;
const TOUCH_X_MAX: i32 = 3700;
const TOUCH_Y_MIN: i32 = 200;
const TOUCH_Y_MAX: i32 = 3800;

// Minimum Z1 (pressure) reading to consider a touch valid
const TOUCH_Z_THRESHOLD: u16 = 100;

fn main() -> Result<()> {
    // Required for some ESP-IDF runtime patches to link correctly
    esp_idf_svc::sys::link_patches();
    // Bridge the `log` crate to ESP-IDF's logging subsystem
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("ESP32 Display Demo starting…");

    let peripherals = Peripherals::take()?;
    let pins = peripherals.pins;

    // ── GPIO assignments ──────────────────────────────────────────────────
    // SPI bus (shared between ST7789 and XPT2046)
    let sclk = pins.gpio14; // TFT_SCLK / T_CLK
    let mosi = pins.gpio13; // TFT_MOSI / T_MOSI
    let miso = pins.gpio12; // TFT_MISO / T_MISO
    // Display-only signals
    let cs_display = pins.gpio15; // TFT_CS
    let dc_gpio = pins.gpio2; // TFT_DC (Data / Command select)
    let bl_gpio = pins.gpio27; // TFT_BL (Backlight)
    // Touch-only signals
    let cs_touch = pins.gpio33; // T_CS
    let irq_gpio = pins.gpio36; // T_IRQ  (active-LOW when touched)

    // ── Shared SPI driver (VSPI / SPI2) ──────────────────────────────────
    let spi_driver = SpiDriver::new(
        peripherals.spi2,
        sclk,
        mosi,
        Some(miso),
        &SpiDriverConfig::new(),
    )?;

    // Display device: 40 MHz (ST7789 supports up to ~80 MHz, 40 MHz is safe)
    let display_spi = SpiDeviceDriver::new(
        &spi_driver,
        Some(cs_display),
        &SpiConfig::new().baudrate(40u32.MHz()),
    )?;

    // Touch device: 2 MHz (XPT2046 maximum ~2.5 MHz)
    let mut touch_spi = SpiDeviceDriver::new(
        &spi_driver,
        Some(cs_touch),
        &SpiConfig::new().baudrate(2u32.MHz()),
    )?;

    // ── GPIO outputs ──────────────────────────────────────────────────────
    let dc = PinDriver::output(dc_gpio)?; // Data/Command for display
    let mut bl = PinDriver::output(bl_gpio)?;
    bl.set_high()?; // Turn on backlight

    // ── GPIO input ────────────────────────────────────────────────────────
    let irq = PinDriver::input(irq_gpio)?; // Touch IRQ (active LOW)

    // ── Initialise ST7789 display ─────────────────────────────────────────
    // CS is managed automatically by SpiDeviceDriver, so SPIInterface only
    // needs the SPI device handle and the DC pin.
    let di = SPIInterface::new(display_spi, dc);

    let mut delay = FreeRtos;
    let mut display = Builder::new(ST7789, di)
        .display_size(SCREEN_WIDTH as u16, SCREEN_HEIGHT as u16)
        // Most ST7789 240×320 panels need colour inversion for correct colours
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::Portrait(false))
        .init(&mut delay)
        .map_err(|_| anyhow::anyhow!("Failed to initialise ST7789 display"))?;

    info!("Display initialised");

    // ── Initial screen content ────────────────────────────────────────────
    display
        .clear(Rgb565::WHITE)
        .map_err(|_| anyhow::anyhow!("Failed to clear display"))?;

    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::BLACK);

    // "Hello, world!" centred at x=120, y=30 (as required)
    Text::with_alignment(
        "Hello, world!",
        Point::new(120, 30),
        text_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .map_err(|_| anyhow::anyhow!("Failed to draw text"))?;

    // "Touch screen to test" centred on screen
    Text::with_alignment(
        "Touch screen to test",
        Point::new(SCREEN_WIDTH / 2, SCREEN_HEIGHT / 2),
        text_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .map_err(|_| anyhow::anyhow!("Failed to draw text"))?;

    info!("UI ready – waiting for touch input…");

    // ── Main loop ─────────────────────────────────────────────────────────
    loop {
        // T_IRQ goes LOW while the screen is being touched
        if irq.is_low() {
            if let Some((x, y, z)) = read_touch(&mut touch_spi) {
                // Serial monitor output (UART0, 115200 baud as per sdkconfig.defaults)
                info!("Touch – X: {}, Y: {}, Pressure: {}", x, y, z);

                // Update the display (errors are non-fatal here)
                let _ = print_touch_to_display(&mut display, x, y, z);
            }
            // Debounce – matches the 100 ms delay in the original Arduino sketch
            FreeRtos::delay_ms(100u32);
        }

        // Yield to other FreeRTOS tasks
        FreeRtos::delay_ms(10u32);
    }
}

// ── XPT2046 helpers ──────────────────────────────────────────────────────────

/// Read X, Y and pressure from the XPT2046 and map them to screen coordinates.
///
/// Returns `(screen_x, screen_y, pressure)` or `None` when the reading is
/// below the pressure threshold (i.e. no valid touch).
fn read_touch<SPI>(spi: &mut SPI) -> Option<(i32, i32, i32)>
where
    SPI: embedded_hal::spi::SpiDevice,
{
    let raw_x = xpt2046_read(spi, XPT2046_CMD_X).ok()?;
    let raw_y = xpt2046_read(spi, XPT2046_CMD_Y).ok()?;
    let z1 = xpt2046_read(spi, XPT2046_CMD_Z1).ok()?;

    if z1 < TOUCH_Z_THRESHOLD {
        return None;
    }

    // The XPT2046 X axis corresponds to the display Y axis and vice-versa
    // (same mapping as in the original Arduino sketch).
    let x = map_range(raw_y as i32, TOUCH_Y_MIN, TOUCH_Y_MAX, 1, SCREEN_HEIGHT);
    let y = map_range(raw_x as i32, TOUCH_X_MIN, TOUCH_X_MAX, 1, SCREEN_WIDTH);

    Some((x, y, z1 as i32))
}

/// Send a 1-byte command to the XPT2046 and read back the 12-bit ADC result.
///
/// The XPT2046 uses a simple SPI protocol:
///   - Byte 0 (TX): command byte  
///   - Bytes 1-2 (RX): 12-bit result in bits [14:3] of the 16-bit response
fn xpt2046_read<SPI>(spi: &mut SPI, cmd: u8) -> Result<u16, SPI::Error>
where
    SPI: embedded_hal::spi::SpiDevice,
{
    let mut buf = [cmd, 0x00, 0x00];
    spi.transfer_in_place(&mut buf)?;
    // The 12-bit result occupies bits 14..3 of the two response bytes
    let value = ((buf[1] as u16) << 8 | buf[2] as u16) >> 3;
    Ok(value & 0x0FFF)
}

// ── Display helpers ───────────────────────────────────────────────────────────

/// Display X, Y and pressure in the lower area of the screen.
///
/// Clears the previous reading before drawing so old values do not linger.
fn print_touch_to_display<D>(display: &mut D, x: i32, y: i32, z: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    // Clear the touch-data area (rows 190-309)
    let clear_style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::WHITE)
        .build();
    Rectangle::new(
        Point::new(0, 190),
        Size::new(SCREEN_WIDTH as u32, 120),
    )
    .into_styled(clear_style)
    .draw(display)?;

    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::BLACK);
    let cx = SCREEN_WIDTH / 2;

    Text::with_alignment(
        &format!("X = {}", x),
        Point::new(cx, 215),
        text_style,
        Alignment::Center,
    )
    .draw(display)?;

    Text::with_alignment(
        &format!("Y = {}", y),
        Point::new(cx, 245),
        text_style,
        Alignment::Center,
    )
    .draw(display)?;

    Text::with_alignment(
        &format!("Pressure = {}", z),
        Point::new(cx, 275),
        text_style,
        Alignment::Center,
    )
    .draw(display)?;

    Ok(())
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Re-map `value` from [`in_min`, `in_max`] to [`out_min`, `out_max`].
///
/// Equivalent to Arduino's `map()` function, with clamping to the output
/// range so out-of-bounds ADC readings never produce negative pixel indices.
fn map_range(value: i32, in_min: i32, in_max: i32, out_min: i32, out_max: i32) -> i32 {
    if in_max == in_min {
        return out_min;
    }
    let result = (value - in_min) * (out_max - out_min) / (in_max - in_min) + out_min;
    result.clamp(out_min.min(out_max), out_min.max(out_max))
}
