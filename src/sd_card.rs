use embedded_sdmmc::{Controller, Directory, TimeSource, Timestamp, Volume, VolumeIdx};
use stm32h7xx_hal::hal;
use stm32h7xx_hal::time::U32Ext;
use stm32h7xx_hal::{
    device::SDMMC1,
    sdmmc::{Sdmmc, SdmmcBlockDevice},
};

use crate::lcd::Lcd;
use crate::rprintln;

struct FakeTime;

impl TimeSource for FakeTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 52, //2022
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 1,
        }
    }
}

pub struct SdCard {
    sd_card: Controller<SdmmcBlockDevice<Sdmmc<SDMMC1>>, FakeTime>,
    fat_volume: Volume,
    fat_root_dir: Directory,
}

impl SdCard {
    pub fn new(mut sd_card: Sdmmc<SDMMC1>) -> Self {
        // setup connection to SD card with 50MHz
        if let Ok(_) = sd_card.init_card(U32Ext::mhz(50)) {
            let mut sd_card = Controller::new(sd_card.sdmmc_block_device(), FakeTime);
            if let Ok(fat_volume) = sd_card.get_volume(VolumeIdx(0)) {
                if let Ok(fat_root_dir) = sd_card.open_root_dir(&fat_volume) {
                    SdCard {
                        sd_card,
                        fat_volume,
                        fat_root_dir,
                    }
                } else {
                    rprintln!("Couldn't open root directory!");
                    panic!();
                }
            } else {
                rprintln!("Couldn't grad volume 0!");
                panic!();
            }
        } else {
            rprintln!("Couldn't init sd card!");
            panic!();
        }
    }

    pub fn write_file_in_sdram<SPI, DC, CS, RESET>(
        &mut self,
        file_name: &str,
        lcd: &mut Lcd<SPI, DC, CS, RESET>,
        sdram: &mut [f32],
    ) -> usize
    where
        SPI: hal::blocking::spi::Write<u8>,
        DC: hal::digital::v2::OutputPin,
        CS: hal::digital::v2::OutputPin,
        RESET: hal::digital::v2::OutputPin,
    {
        let file_length_in_samples;

        let mut file = self
            .sd_card
            .open_file_in_dir(
                &mut self.fat_volume,
                &self.fat_root_dir,
                file_name,
                embedded_sdmmc::Mode::ReadOnly,
            )
            .unwrap();

        let file_length_in_bytes = file.length() as usize;
        file_length_in_samples = file_length_in_bytes / core::mem::size_of::<f32>();

        // load wave file in chunks of CHUNK_SIZE samples into sdram

        lcd.draw_loading_bar(0, file_name);

        const CHUNK_SIZE: usize = 10_000; // has to be a multiple of 4, bigger chunks mean faster loading times
        let chunk_iterator = file_length_in_bytes / CHUNK_SIZE;
        file.seek_from_start(2).unwrap(); // offset the reading of the chunks

        for i in 0..chunk_iterator {
            let mut chunk_buffer = [0u8; CHUNK_SIZE];

            self.sd_card
                .read(&self.fat_volume, &mut file, &mut chunk_buffer)
                .unwrap();

            for k in 0..CHUNK_SIZE {
                // converting every word consisting of four u8 into f32 in buffer
                if k % 4 == 0 {
                    let f32_buffer = [
                        chunk_buffer[k],
                        chunk_buffer[k + 1],
                        chunk_buffer[k + 2],
                        chunk_buffer[k + 3],
                    ];
                    let iterator = i * (CHUNK_SIZE / 4) + k / 4;
                    sdram[iterator] = f32::from_le_bytes(f32_buffer);
                }
            }

            lcd.draw_loading_bar(
                ((i as f32 / chunk_iterator as f32) * 100.0) as u32,
                file_name,
            );
        }

        file_length_in_samples
    }
}
