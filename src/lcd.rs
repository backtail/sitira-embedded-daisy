use display_interface_spi::SPIInterface;
use ili9341::{DisplaySize240x320, Ili9341, Orientation};
use stm32h7xx_hal::hal;

use embedded_graphics::{
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
};

pub struct Lcd<SPI, DC, CS, RESET> {
    driver: Ili9341<SPIInterface<SPI, DC, CS>, RESET>,
}

impl<SPI, DC, CS, RESET> Lcd<SPI, DC, CS, RESET>
where
    SPI: hal::blocking::spi::Write<u8>,
    DC: hal::digital::v2::OutputPin,
    CS: hal::digital::v2::OutputPin,
    RESET: hal::digital::v2::OutputPin,
{
    pub fn new<DELAY>(spi: SPI, dc: DC, cs: CS, reset: RESET, mut delay: DELAY) -> Self
    where
        DELAY: libdaisy::prelude::_embedded_hal_blocking_delay_DelayMs<u16>,
    {
        let interface = SPIInterface::new(spi, dc, cs);

        let driver = Ili9341::new(
            interface,
            reset,
            &mut delay,
            Orientation::Landscape,
            DisplaySize240x320,
        )
        .unwrap();

        Self { driver }
    }

    pub fn setup(&mut self) {
        self.driver.clear(Rgb565::BLACK).unwrap();

        let character_style = MonoTextStyle::new(&ascii::FONT_10X20, Rgb565::WHITE);

        let middle_x: i32 = (self.driver.width() / 2) as i32;
        let middle_y: i32 = (self.driver.height() / 2) as i32;

        let start_text = "Sitira Synth\nby Max Genson\n\nWritten in Rust";
        let position = Point::new(middle_x, middle_y - ((4 * 22) / 2));

        Text::with_alignment(start_text, position, character_style, Alignment::Center)
            .draw(&mut self.driver)
            .unwrap();
    }

    pub fn draw_loading_bar(&mut self, percentage: u32, filename: &str) {
        if percentage == 0 {
            let border_style = PrimitiveStyleBuilder::new()
                .stroke_color(Rgb565::WHITE)
                .stroke_width(3)
                .build();

            let position = Point::new(40, 200);

            // border
            Rectangle::new(
                position,
                Size {
                    width: 240,
                    height: 20,
                },
            )
            .into_styled(border_style)
            .draw(&mut self.driver)
            .unwrap();

            let character_style = MonoTextStyle::new(&ascii::FONT_6X9, Rgb565::WHITE);

            let position = Point::new((self.driver.width() / 2) as i32, 190);

            Text::with_alignment(filename, position, character_style, Alignment::Center)
                .draw(&mut self.driver)
                .unwrap();
        }

        let loading_bar_style = PrimitiveStyleBuilder::new()
            .fill_color(Rgb565::WHITE)
            .build();

        if percentage <= 100 {
            let position = Point::new(46, 206);

            Rectangle::new(
                position,
                Size {
                    width: (231 * percentage) / 100,
                    height: 8,
                },
            )
            .into_styled(loading_bar_style)
            .draw(&mut self.driver)
            .unwrap();
        }
    }
}
