#![no_main]
#![no_std]
#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use log::info;

    use embedded_sdmmc::{Controller, TimeSource, Timestamp, VolumeIdx};
    use libdaisy::{
        audio, logger,
        prelude::*,
        sdmmc,
        system::{self, System},
    };

    use stm32h7xx_hal::pac;
    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
    }

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

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        // initiate system
        let system = system::System::init(ctx.core, ctx.device);

        // setting up core
        let rcc_p = unsafe { pac::Peripherals::steal().RCC };
        let pwr_p = unsafe { pac::Peripherals::steal().PWR };
        let syscfg_p = unsafe { pac::Peripherals::steal().SYSCFG };
        let mut ccdr = System::init_clocks(pwr_p, rcc_p, &syscfg_p);

        // setting up SD card connection
        let sdmmc_d = unsafe { pac::Peripherals::steal().SDMMC1 };
        let mut gpios = system.gpio;
        let mut sd = sdmmc::init(
            gpios.daisy1.unwrap(),
            gpios.daisy2.unwrap(),
            gpios.daisy3.unwrap(),
            gpios.daisy4.unwrap(),
            gpios.daisy5.unwrap(),
            gpios.daisy6.unwrap(),
            sdmmc_d,
            ccdr.peripheral.SDMMC1,
            &mut ccdr.clocks,
        );

        // initiate SD card connection
        gpios.led.set_low().unwrap();
        if let Ok(_) = sd.init_card(50.mhz()) {
            info!("Got SD Card!");
            let mut sd_fatfs = Controller::new(sd.sdmmc_block_device(), FakeTime);
            if let Ok(sd_fatfs_volume) = sd_fatfs.get_volume(VolumeIdx(0)) {
                if let Ok(sd_fatfs_root_dir) = sd_fatfs.open_root_dir(&sd_fatfs_volume) {
                    sd_fatfs
                        .iterate_dir(&sd_fatfs_volume, &sd_fatfs_root_dir, |entry| {
                            info!("{:?}", entry);
                        })
                        .unwrap();
                    sd_fatfs.close_dir(&sd_fatfs_volume, sd_fatfs_root_dir);
                    gpios.led.set_high().unwrap();
                } else {
                    info!("Failed to get root dir");
                }
            } else {
                info!("Failed to get volume 0");
            }
        } else {
            info!("Failed to init SD Card");
        }

        // check sdram
        let sdram = system.sdram;

        let sdram_size_bytes = libdaisy::sdram::Sdram::bytes();
        let sdram_size = sdram_size_bytes / core::mem::size_of::<u32>();

        info!(
            "SDRAM size: {} bytes, {} words starting at {:?}",
            sdram_size_bytes, sdram_size, &sdram[0] as *const _
        );

        // audio buffer
        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];

        info!("Startup done!");

        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
            },
            init::Monotonics(),
        )
    }

    // Non-default idle ensures chip doesn't go to sleep which causes issues for
    // probe.rs currently
    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        loop {
            cortex_m::asm::nop();
        }
    }

    // Interrupt handler for audio
    #[task(binds = DMA1_STR1, local = [audio, buffer], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;

        if audio.get_stereo(buffer) {
            for (left, right) in buffer {
                audio.push_stereo((*left, *right)).unwrap();
            }
        } else {
            info!("Error reading data!");
        }
    }
}
