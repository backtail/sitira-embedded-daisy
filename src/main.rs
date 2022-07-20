#![no_main]
#![no_std]
#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use log::{error, info};

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
        sdram: &'static mut [f32],
        playhead: usize,
        file_length_in_samples: usize,
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
        sdram.fill(0.0);

        let sdram_size_bytes = libdaisy::sdram::Sdram::bytes();
        let sdram_size = sdram_size_bytes / core::mem::size_of::<f32>();
        let sdram_address = core::ptr::addr_of!(sdram[0]);

        info!(
            "SDRAM size: {} bytes, {} words starting at {:?}",
            sdram_size_bytes, sdram_size, sdram_address
        );

        let file_name = "KICADI~1.WAV";
        let mut file_length_in_samples = 0;

        // initiate SD card connection
        if let Ok(_) = sd.init_card(50.mhz()) {
            info!("Got SD Card!");
            let mut sd_card = Controller::new(sd.sdmmc_block_device(), FakeTime);
            if let Ok(mut fat_volume) = sd_card.get_volume(VolumeIdx(0)) {
                if let Ok(fat_root_dir) = sd_card.open_root_dir(&fat_volume) {
                    let mut file = sd_card
                        .open_file_in_dir(
                            &mut fat_volume,
                            &fat_root_dir,
                            file_name,
                            embedded_sdmmc::Mode::ReadOnly,
                        )
                        .unwrap();

                    let file_length_in_bytes = file.length() as usize;
                    file_length_in_samples = file_length_in_bytes / core::mem::size_of::<f32>();
                    info!(
                        "Open file KICADI~1.WAV!, length: {} MB, {} bytes, {} samples",
                        (file_length_in_bytes / 1048576) as f32,
                        file_length_in_bytes,
                        file_length_in_samples,
                    );

                    // load wave file in chunks of CHUNK_SIZE samples into sdram

                    const CHUNK_SIZE: usize = 24_000; // has to be a multiple of 4, bigger chunks mean faster loading times
                    let chunk_iterator = file_length_in_bytes / CHUNK_SIZE;
                    file.seek_from_start(2).unwrap(); // offset the reading of the chunks

                    info!(
                        "Loading in {} chunks of {} samples",
                        chunk_iterator, CHUNK_SIZE
                    );

                    for i in 0..chunk_iterator {
                        let mut chunk_buffer = [0u8; CHUNK_SIZE]; 

                        sd_card
                            .read(&fat_volume, &mut file, &mut chunk_buffer)
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

                        match i {
                            _ if i == 0 => info!("0%"),
                            _ if i == (chunk_iterator / 10) => info!("10%"),
                            _ if i == (chunk_iterator / 4) => info!("25%"),
                            _ if i == (chunk_iterator / 2) => info!("50%"),
                            _ if i == ((3 * chunk_iterator) / 4) => info!("75%"),
                            _ if i == chunk_iterator - 1 => info!("100%"),
                            _ => (),
                        }
                    }

                    info!("All chunks loaded!");

                    sd_card.close_dir(&fat_volume, fat_root_dir);
                } else {
                    info!("Failed to get root dir");
                    core::panic!();
                }
            } else {
                info!("Failed to get volume 0");
                core::panic!();
            }
        } else {
            error!("Failed to init SD Card");
            core::panic!();
        }

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX]; // audio ring buffer
        let playhead = 457; // skip wav header information poorly

        // // for debugging purposes

        // info!("SDRAM contents!");

        // let offset: usize = 457;
        // let range: usize = 20;
        // let end = offset + range;

        // for n in offset..end {
        //     let chunk_buffer = sdram[n].to_le_bytes();
        //     info!(
        //         "Offset {}: {:#04x}, {:#04x}, {:#04x}, {:#04x}, {:?}",
        //         n, chunk_buffer[0], chunk_buffer[1], chunk_buffer[2], chunk_buffer[3], sdram[n]
        //     );
        // }

        info!("Startup done!");

        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
                sdram,
                playhead,
                file_length_in_samples,
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
    #[task(binds = DMA1_STR1, local = [audio, buffer, playhead, sdram, file_length_in_samples, index: usize = 0], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let mut buffer = *ctx.local.buffer;
        let sdram: &mut [f32] = *ctx.local.sdram;
        let mut index = *ctx.local.index + *ctx.local.playhead;

        audio.get_stereo(&mut buffer);
        for (_left, _right) in buffer {
            let mono = sdram[index] * 0.5;
            audio.push_stereo((mono, mono)).unwrap();
            index += 1;
        }

        if *ctx.local.playhead < *ctx.local.file_length_in_samples {
            *ctx.local.playhead += audio::BLOCK_SIZE_MAX;
        } else {
            info!("Now playing again from start");
            *ctx.local.playhead = 457;
        }
    }
}
