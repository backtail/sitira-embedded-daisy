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
    struct Shared {
        counter: usize,
    }

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        sample_buffer: [f32; BUFFER_COUNT],
    }

    const BUFFER_COUNT: usize = 6_000;

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
        let gpios = system.gpio;
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

        // check sdram
        let sdram = system.sdram;

        let sdram_size_bytes = libdaisy::sdram::Sdram::bytes();
        let sdram_size = sdram_size_bytes / core::mem::size_of::<u32>();

        info!(
            "SDRAM size: {} bytes, {} words starting at {:?}",
            sdram_size_bytes, sdram_size, &sdram[0] as *const _
        );

        let mut integer_buffer: [u8; BUFFER_COUNT] = [0; BUFFER_COUNT];                 // u8 sample buffer

        let file_name = "KICADI~1.WAV";

        // initiate SD card connection
        if let Ok(_) = sd.init_card(50.mhz()) {
            info!("Got SD Card!");
            let mut sd_fatfs = Controller::new(sd.sdmmc_block_device(), FakeTime);
            if let Ok(mut sd_fatfs_volume) = sd_fatfs.get_volume(VolumeIdx(0)) {
                if let Ok(sd_fatfs_root_dir) = sd_fatfs.open_root_dir(&sd_fatfs_volume) {
                    let mut sample_on_sd_card = sd_fatfs
                        .open_file_in_dir(
                            &mut sd_fatfs_volume,
                            &sd_fatfs_root_dir,
                            file_name,
                            embedded_sdmmc::Mode::ReadOnly,
                        )
                        .unwrap();

                    let sample_length = sample_on_sd_card.length();
                    info!(
                        "Open file KICADI~1.WAV!, length: {} MB",
                        (sample_length / 1048576) as f32
                    );

                    // store wav file in flash
                    sd_fatfs
                        .read(
                            &mut sd_fatfs_volume,
                            &mut sample_on_sd_card,
                            &mut integer_buffer,
                        )
                        .unwrap();
                    sd_fatfs.close_dir(&sd_fatfs_volume, sd_fatfs_root_dir);
                } else {
                    info!("Failed to get root dir");
                }
            } else {
                info!("Failed to get volume 0");
            }
        } else {
            info!("Failed to init SD Card");
        }

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];               // audio ring buffer
        let mut sample_buffer = [0.0; BUFFER_COUNT];                           // f32 sample buffer

        // convert u8 sample buffer into f32
        let range = sample_buffer.iter().skip(1).step_by(2).count();
        for n in 0..range {
            sample_buffer[n] =
                ((u32::from_be_bytes([0, 0, integer_buffer[2 * n], integer_buffer[2 * n + 1]])
                    as i32
                    - 32768) as f32)
                    / 65536.0;
        }
        let counter = 0;

        info!(
            "Output WAV file sample from position 2000 to 2050 to check, if any audio is present: {:?}",
            &integer_buffer[2_000..2_050]
        );

        info!("Startup done!");

        (
            Shared { counter },
            Local {
                audio: system.audio,
                buffer,
                sample_buffer,
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
    #[task(binds = DMA1_STR1, local = [audio, buffer, sample_buffer], shared = [counter], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = ctx.local.audio;

        let buffer = ctx.local.buffer;

        let sample_buffer = ctx.local.sample_buffer;

        ctx.shared.counter.lock(|counter| {
            if audio.get_stereo(buffer) {
                for (_left, _right) in buffer {
                    audio
                        .push_stereo((sample_buffer[*counter], sample_buffer[*counter]))
                        .unwrap();

                    *counter += 1;
                }
            } else {
                info!("Error reading data!");
            }

            if *counter > 3_000 {
                *counter = 0;
            }
        });
    }
}
