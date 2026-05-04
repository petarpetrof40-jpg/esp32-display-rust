//! ESP32-2432S024 display + touchscreen demo
//!
//! Hardware: ESP32-WROOM-32D, ST7789 240×320 display, XPT2046 touchscreen
//!
//! GPIO pin mapping:
//!   Display (ST7789) via SPI2 (HSPI):
//!     TFT_MISO  GPIO 12
//!     TFT_MOSI  GPIO 13
//!     TFT_SCLK  GPIO 14
//!     TFT_CS    GPIO 15
//!     TFT_DC    GPIO 2
//!     TFT_RST   -1 (not used, internally connected)
//!     TFT_BL    GPIO 27 (backlight, active HIGH)
//!
//!   Touch (XPT2046) shares the same SPI2 bus:
//!     T_CS      GPIO 33
//!     T_IRQ     GPIO 36

use anyhow::Result;
use display_interface_spi::SPIInterface;
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
};
use embedded_hal::digital::InputPin;
use embedded_hal::spi::SpiDevice;
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::PinDriver,
    peripherals::Peripherals,
    spi::{
        config::{Config as SpiConfig, DriverConfig},
        SpiDeviceDriver, SpiDriver,
    },
    units::FromValueType,
};
use mipidsi::{
    models::ST7789,
    options::{ColorInversion, ColorOrder},
    Builder,
};

// ---------------------------------------------------------------------------
// Screen geometry
// ---------------------------------------------------------------------------

const SCREEN_WIDTH: i32 = 240;
const SCREEN_HEIGHT: i32 = 320;

// ---------------------------------------------------------------------------
// XPT2046 touch calibration constants
// (raw ADC range → screen pixel range)
//
// Calibration mapping from the original Arduino sketch:
//   screen_x = map(raw_y, 200, 3800, 1, SCREEN_HEIGHT)
//   screen_y = map(raw_x, 240, 3700, 1, SCREEN_WIDTH)
// ---------------------------------------------------------------------------

const TOUCH_Y_RAW_MIN: u16 = 200;
const TOUCH_Y_RAW_MAX: u16 = 3800;
const TOUCH_X_RAW_MIN: u16 = 240;
const TOUCH_X_RAW_MAX: u16 = 3700;

// ---------------------------------------------------------------------------
// XPT2046 SPI command bytes
// Format: START(1) | A2:A0 | MODE(0=12bit) | SER/DFR(0=diff) | PD1:PD0
// ---------------------------------------------------------------------------

/// Differential 12-bit X position command
const XPT_CMD_X: u8 = 0xD0;
/// Differential 12-bit Y position command
const XPT_CMD_Y: u8 = 0x90;
/// Differential 12-bit Z1 pressure command
const XPT_CMD_Z1: u8 = 0xB0;

// ---------------------------------------------------------------------------
// Touch data structure
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct TouchPoint {
    /// Raw ADC X value (0–4095)
    x: u16,
    /// Raw ADC Y value (0–4095)
    y: u16,
    /// Pressure Z1 value (0–4095; higher = pressed harder)
    z: u16,
}

// ---------------------------------------------------------------------------
// XPT2046 driver
// ---------------------------------------------------------------------------

/// Minimal XPT2046 touchscreen driver over an `embedded-hal 1.x` SPI device.
struct Xpt2046<SPI, IRQ> {
    spi: SPI,
    irq: IRQ,
}

impl<SPI, IRQ> Xpt2046<SPI, IRQ>
where
    SPI: SpiDevice,
    IRQ: InputPin,
{
    fn new(spi: SPI, irq: IRQ) -> Self {
        Self { spi, irq }
    }

    /// Returns `true` when the IRQ line is asserted (active LOW).
    fn is_touched(&mut self) -> bool {
        self.irq.is_low().unwrap_or(false)
    }

    /// Send a single-byte command and read the 12-bit ADC result.
    ///
    /// XPT2046 SPI transaction (3 bytes):
    ///   MOSI: [CMD] [0x00] [0x00]
    ///   MISO: [----] [HIGH] [LOW ]
    /// The 12-bit result sits in bits [14:3] of the 16-bit MISO word.
    fn read_channel(&mut self, cmd: u8) -> Result<u16> {
        let mut buf = [cmd, 0x00, 0x00];
        self.spi
            .transfer_in_place(&mut buf)
            .map_err(|_| anyhow::anyhow!("SPI transfer failed"))?;
        let raw = ((buf[1] as u16) << 8 | buf[2] as u16) >> 3;
        Ok(raw & 0x0FFF)
    }

    /// Read the current touch position.  Returns `None` when the screen is
    /// not being touched or the pressure reading is too low.
    fn read_touch(&mut self) -> Result<Option<TouchPoint>> {
        if !self.is_touched() {
            return Ok(None);
        }

        // Read Z1 pressure first; if too low the touch is spurious.
        let z = self.read_channel(XPT_CMD_Z1)?;
        if z < 10 {
            return Ok(None);
        }

        let x = self.read_channel(XPT_CMD_X)?;
        let y = self.read_channel(XPT_CMD_Y)?;

        Ok(Some(TouchPoint { x, y, z }))
    }
}

// ---------------------------------------------------------------------------
// Coordinate mapping helper (Arduino map() equivalent)
// ---------------------------------------------------------------------------

/// Map `value` from the range [in_min, in_max] to [out_min, out_max].
/// The value is clamped to [in_min, in_max] before mapping.
fn map_range(value: u16, in_min: u16, in_max: u16, out_min: i32, out_max: i32) -> i32 {
    let value = value.clamp(in_min, in_max) as i64;
    let in_min = in_min as i64;
    let in_max = in_max as i64;
    let out_min = out_min as i64;
    let out_max = out_max as i64;
    ((value - in_min) * (out_max - out_min) / (in_max - in_min) + out_min) as i32
}

// ---------------------------------------------------------------------------
// Application entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    // Link esp-idf patches and initialise the default logger (UART0, 115200).
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("ESP32 display/touch demo starting");

    let peripherals = Peripherals::take()?;

    // -----------------------------------------------------------------------
    // GPIO pin assignments
    // -----------------------------------------------------------------------

    let tft_sclk = peripherals.pins.gpio14;
    let tft_mosi = peripherals.pins.gpio13;
    let tft_miso = peripherals.pins.gpio12;
    let tft_cs = peripherals.pins.gpio15;
    let tft_dc = peripherals.pins.gpio2;
    let tft_bl = peripherals.pins.gpio27;
    let t_cs = peripherals.pins.gpio33;
    let t_irq = peripherals.pins.gpio36;

    // Turn on the backlight (active HIGH).
    let mut backlight = PinDriver::output(tft_bl)?;
    backlight.set_high()?;
    log::info!("Backlight on");

    // -----------------------------------------------------------------------
    // Shared SPI2 (HSPI) bus
    //
    // Both the display and the touch controller share MOSI / MISO / SCLK.
    // Each device gets its own SpiDeviceDriver with its own CS pin and clock
    // rate.  esp-idf-hal 0.43 allows multiple SpiDeviceDriver instances on
    // the same SpiDriver (T: Borrow<SpiDriver<'d>>).
    // -----------------------------------------------------------------------

    let spi_bus = SpiDriver::new(
        peripherals.spi2,
        tft_sclk,
        tft_mosi,
        Some(tft_miso),
        &DriverConfig::new(),
    )?;

    // Display SPI device at 40 MHz.
    let display_spi = SpiDeviceDriver::new(
        &spi_bus,
        Some(tft_cs),
        &SpiConfig::new().baudrate(40u32.MHz().into()),
    )?;

    // Touch SPI device at 2.5 MHz (XPT2046 maximum).
    let touch_spi = SpiDeviceDriver::new(
        &spi_bus,
        Some(t_cs),
        &SpiConfig::new().baudrate(2_500_000u32.into()),
    )?;

    // -----------------------------------------------------------------------
    // Display initialisation (ST7789, BGR colour order, no inversion)
    // -----------------------------------------------------------------------

    let dc_pin = PinDriver::output(tft_dc)?;
    let di = SPIInterface::new(display_spi, dc_pin);

    let mut delay = FreeRtos;
    let mut display = Builder::new(ST7789, di)
        .display_size(SCREEN_WIDTH as u16, SCREEN_HEIGHT as u16)
        .color_order(ColorOrder::Bgr)
        .invert_colors(ColorInversion::Normal)
        .init(&mut delay)
        .map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;

    log::info!("Display initialised");

    // Fill the screen white.
    display
        .clear(Rgb565::WHITE)
        .map_err(|e| anyhow::anyhow!("Display clear failed: {:?}", e))?;

    // -----------------------------------------------------------------------
    // Draw static text
    // -----------------------------------------------------------------------

    let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::BLACK);
    let center_x = SCREEN_WIDTH / 2;

    // "Hello, world!" – centred near the top.
    Text::with_alignment(
        "Hello, world!",
        Point::new(center_x, 30),
        text_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .map_err(|e| anyhow::anyhow!("Draw text failed: {:?}", e))?;

    // "Touch screen to test" – centred in the middle of the screen.
    Text::with_alignment(
        "Touch screen to test",
        Point::new(center_x, SCREEN_HEIGHT / 2),
        text_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .map_err(|e| anyhow::anyhow!("Draw text failed: {:?}", e))?;

    log::info!("Static text drawn – waiting for touch input");

    // -----------------------------------------------------------------------
    // Touch controller initialisation
    // -----------------------------------------------------------------------

    let touch_irq = PinDriver::input(t_irq)?;
    let mut touch = Xpt2046::new(touch_spi, touch_irq);

    // -----------------------------------------------------------------------
    // Main loop
    // -----------------------------------------------------------------------

    // Clear-rect style (white fill, no outline).
    let clear_style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb565::WHITE)
        .build();

    // Y-positions for the three data lines (below the centre prompt).
    let y_base = SCREEN_HEIGHT / 2 + 40;
    let line_height = 25;

    loop {
        if let Some(pt) = touch.read_touch()? {
            // ------------------------------------------------------------------
            // Calibrate raw ADC values → screen coordinates.
            //
            //   screen_x = map(pt.y, 200, 3800, 1, SCREEN_HEIGHT)
            //   screen_y = map(pt.x, 240, 3700, 1, SCREEN_WIDTH)
            // ------------------------------------------------------------------
            let screen_x = map_range(pt.y, TOUCH_Y_RAW_MIN, TOUCH_Y_RAW_MAX, 1, SCREEN_HEIGHT);
            let screen_y = map_range(pt.x, TOUCH_X_RAW_MIN, TOUCH_X_RAW_MAX, 1, SCREEN_WIDTH);

            // Log to serial monitor.
            log::info!(
                "Touch – X: {}, Y: {}, Pressure: {}",
                screen_x,
                screen_y,
                pt.z
            );

            // Clear the data area.
            Rectangle::new(
                Point::new(0, y_base - 20),
                Size::new(SCREEN_WIDTH as u32, (line_height * 3 + 30) as u32),
            )
            .into_styled(clear_style)
            .draw(&mut display)
            .map_err(|e| anyhow::anyhow!("Draw rect failed: {:?}", e))?;

            // Display X coordinate.
            let x_str = format!("X: {}", screen_x);
            Text::with_alignment(
                &x_str,
                Point::new(center_x, y_base),
                text_style,
                Alignment::Center,
            )
            .draw(&mut display)
            .map_err(|e| anyhow::anyhow!("Draw text failed: {:?}", e))?;

            // Display Y coordinate.
            let y_str = format!("Y: {}", screen_y);
            Text::with_alignment(
                &y_str,
                Point::new(center_x, y_base + line_height),
                text_style,
                Alignment::Center,
            )
            .draw(&mut display)
            .map_err(|e| anyhow::anyhow!("Draw text failed: {:?}", e))?;

            // Display pressure.
            let z_str = format!("Pressure: {}", pt.z);
            Text::with_alignment(
                &z_str,
                Point::new(center_x, y_base + line_height * 2),
                text_style,
                Alignment::Center,
            )
            .draw(&mut display)
            .map_err(|e| anyhow::anyhow!("Draw text failed: {:?}", e))?;

            // 100 ms debounce / rate-limit delay.
            delay.delay_ms(100);
        }
    }
}
