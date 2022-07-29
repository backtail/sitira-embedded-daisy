#![no_main]
#![no_std]

pub mod encoder;
pub mod lcd;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use embedded_sdmmc::{Controller, TimeSource, Timestamp, VolumeIdx};
    use libdaisy::{
        audio,
        gpio::*,
        hid,
        prelude::{Analog, *},
        sdmmc,
        system::{self, System},
    };
    use stm32h7xx_hal::pac;
    use stm32h7xx_hal::time::U32Ext;
    use stm32h7xx_hal::timer::Timer;
    use stm32h7xx_hal::{
        adc,
        spi::{Mode, NoMiso},
        stm32,
    };

    use micromath::F32Ext;

    use crate::encoder;
    use crate::lcd;

    use biquad::*;

    #[shared]
    struct Shared {
        _pot2_value: f32,
        encoder_value: i32,
        biquad: DirectForm1<f32>,
    }

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        sdram: &'static mut [f32],
        playhead: usize,
        file_length_in_samples: usize,
        adc1: adc::Adc<stm32::ADC1, adc::Enabled>,
        control2: hid::AnalogControl<Daisy15<Analog>>,
        timer2: Timer<stm32::TIM2>,
        led1: Daisy24<Output<PushPull>>,
        switch2: hid::Switch<Daisy28<Input<PullUp>>>,
        encoder: encoder::RotaryEncoder<
            Daisy14<Input<PullUp>>,
            Daisy25<Input<PullUp>>,
            Daisy26<Input<PullUp>>,
        >,
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
        // initiate system
        let mut system = system::System::init(ctx.core, ctx.device);

        // setting up core
        let rcc_p = unsafe { pac::Peripherals::steal().RCC };
        let pwr_p = unsafe { pac::Peripherals::steal().PWR };
        let syscfg_p = unsafe { pac::Peripherals::steal().SYSCFG };
        let mut ccdr = System::init_clocks(pwr_p, rcc_p, &syscfg_p);

        // setting up SD card connection
        let sdmmc_d = unsafe { pac::Peripherals::steal().SDMMC1 };
        let mut sd = sdmmc::init(
            system.gpio.daisy1.unwrap(),
            system.gpio.daisy2.unwrap(),
            system.gpio.daisy3.unwrap(),
            system.gpio.daisy4.unwrap(),
            system.gpio.daisy5.unwrap(),
            system.gpio.daisy6.unwrap(),
            sdmmc_d,
            ccdr.peripheral.SDMMC1,
            &mut ccdr.clocks,
        );

        // fill sdram with zeroes
        let sdram = system.sdram;
        sdram.fill(0.0);

        // graphics

        // setting up SPI1 for ILI9431 driver

        let mut lcd_nss = system
            .gpio
            .daisy7
            .expect("Failed to get pin 7 of the daisy!")
            .into_push_pull_output();

        lcd_nss.set_high().unwrap();

        let lcd_clk = system
            .gpio
            .daisy8
            .expect("Failed to get pin 8 of the daisy!")
            .into_alternate_af5();

        let lcd_miso = NoMiso {};

        let lcd_mosi = system
            .gpio
            .daisy10
            .expect("Failed to get pin 10 of the daisy!")
            .into_alternate_af5()
            .internal_pull_up(true);

        let lcd_dc = system
            .gpio
            .daisy11
            .expect("Failed to get pin 11 of the daisy!")
            .into_push_pull_output();
        let lcd_cs = system
            .gpio
            .daisy12
            .expect("Failed to get pin 12 of the daisy!")
            .into_push_pull_output();

        let lcd_reset = system
            .gpio
            .daisy17
            .expect("Failed to get pin 17 of the daisy!")
            .into_push_pull_output();

        let mode = Mode {
            polarity: stm32h7xx_hal::spi::Polarity::IdleLow,
            phase: stm32h7xx_hal::spi::Phase::CaptureOnFirstTransition,
        };

        let lcd_spi = unsafe { pac::Peripherals::steal().SPI1 }.spi(
            (lcd_clk, lcd_miso, lcd_mosi),
            mode,
            U32Ext::mhz(25),
            ccdr.peripheral.SPI1,
            &ccdr.clocks,
        );

        let timer3 = unsafe { pac::Peripherals::steal().TIM3 }.timer(
            1.ms(),
            ccdr.peripheral.TIM3,
            &ccdr.clocks,
        );
        let delay = stm32h7xx_hal::delay::DelayFromCountDownTimer::new(timer3);

        let mut lcd = lcd::Lcd::new(lcd_spi, lcd_dc, lcd_cs, lcd_reset, delay);

        lcd.setup();
        // for i in 0..100 {
        //     lcd.draw_loading_bar(i, "Kicad-Intro.wav");
        // }

        // setting up SD Card and reading wav files

        let file_name = "KICADI~1.WAV";
        let file_length_in_samples;

        // initiate SD card connection
        if let Ok(_) = sd.init_card(U32Ext::mhz(50)) {
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

                    // load wave file in chunks of CHUNK_SIZE samples into sdram

                    lcd.draw_loading_bar(0, file_name);

                    const CHUNK_SIZE: usize = 48_000; // has to be a multiple of 4, bigger chunks mean faster loading times
                    let chunk_iterator = file_length_in_bytes / CHUNK_SIZE;
                    file.seek_from_start(2).unwrap(); // offset the reading of the chunks

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

                        lcd.draw_loading_bar(
                            ((i as f32 / chunk_iterator as f32) * 100_f32) as u32,
                            file_name,
                        );
                    }

                    sd_card.close_dir(&fat_volume, fat_root_dir);
                } else {
                    lcd.print_error_center(lcd.width / 2, 190, "Failed to get file!");
                    core::panic!();
                }
            } else {
                lcd.print_error_center(lcd.width / 2, 190, "Failed to get volume 0!");
                core::panic!();
            }
        } else {
            lcd.print_error_center(lcd.width / 2, 190, "No SD card found!");
            core::panic!();
        }

        // setting up ADC1 and TIM2

        system.timer2.set_freq(1.ms());

        let mut adc1 = system.adc1.enable();
        adc1.set_resolution(adc::Resolution::SIXTEENBIT);
        let adc1_max_value = adc1.max_sample() as f32;

        let pot2_pin = system
            .gpio
            .daisy15
            .take()
            .expect("Failed to get pin 15 of the daisy!")
            .into_analog();

        let _pot2_value = 0.0_f32;

        let mut control2 = hid::AnalogControl::new(pot2_pin, adc1_max_value);
        control2.set_transform(|x| (x + 1.0).log10() * 2_f32.log10());

        // setting up button input

        let switch2_pin = system
            .gpio
            .daisy28
            .take()
            .expect("Failed to get pin 28 of the daisy!")
            .into_pull_up_input();
        let switch2 = hid::Switch::new(switch2_pin, hid::SwitchType::PullUp);

        let led1 = system
            .gpio
            .daisy24
            .take()
            .expect("Failed to get pin 24 of the daisy!")
            .into_push_pull_output();

        // setting up rotary encoder

        let rotary_switch_pin = system
            .gpio
            .daisy14
            .take()
            .expect("Failed to get pin 14 of the daisy!")
            .into_pull_up_input();

        let rotary_clock_pin = system
            .gpio
            .daisy25
            .take()
            .expect("Failed to get pin 25 of the daisy!")
            .into_pull_up_input();

        let rotary_data_pin = system
            .gpio
            .daisy26
            .take()
            .expect("Failed to get pin 26 of the daisy!")
            .into_pull_up_input();

        let encoder =
            encoder::RotaryEncoder::new(rotary_switch_pin, rotary_clock_pin, rotary_data_pin);

        let encoder_value = 0;

        // setting up biquad filter

        let f0 = biquad::ToHertz::hz(100.0);
        let fs = biquad::ToHertz::khz(48.0);

        let coeffs =
            Coefficients::<f32>::from_params(Type::LowPass, fs, f0, Q_BUTTERWORTH_F32).unwrap();

        let biquad = DirectForm1::<f32>::new(coeffs);

        // audio stuff

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX]; // audio ring buffer
        let playhead = 457; // skip wav header information poorly

        (
            Shared {
                _pot2_value,
                encoder_value,
                biquad,
            },
            Local {
                audio: system.audio,
                buffer,
                sdram,
                playhead,
                file_length_in_samples,
                adc1,
                control2,
                timer2: system.timer2,
                led1,
                switch2,
                encoder,
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
    #[task(binds = DMA1_STR1, local = [audio, buffer, playhead, sdram, file_length_in_samples, index: usize = 0], shared = [biquad], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let mut buffer = *ctx.local.buffer;
        let sdram: &mut [f32] = *ctx.local.sdram;
        let mut index = *ctx.local.index + *ctx.local.playhead;
        let mut biquad = ctx.shared.biquad;

        audio.get_stereo(&mut buffer);
        for (_left, _right) in buffer {
            let mut mono = sdram[index] * 0.7; // multiply with 0.7 for no distortion
            biquad.lock(|biquad| {
                mono = biquad.run(mono);
            });
            audio.push_stereo((mono, mono)).unwrap();
            index += 1;
        }

        if *ctx.local.playhead < *ctx.local.file_length_in_samples {
            *ctx.local.playhead += audio::BLOCK_SIZE_MAX;
        } else {
            *ctx.local.playhead = 457; // very cheap method of skipping the wav file header
        }
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [timer2, adc1, control2, switch2, led1, encoder], shared = [encoder_value, biquad])]
    fn interface_handler(mut ctx: interface_handler::Context) {
        ctx.local.timer2.clear_irq();
        let adc1 = ctx.local.adc1;
        let control2 = ctx.local.control2;

        if let Ok(data) = adc1.read(control2.get_pin()) {
            control2.update(data);
        }

        let mut value = control2.get_value();

        value = value * 20_000.0 + 20.0;

        ctx.shared.biquad.lock(|biquad| {
            biquad.replace_coefficients(
                Coefficients::<f32>::from_params(
                    Type::LowPass,
                    biquad::ToHertz::khz(48.0),
                    biquad::ToHertz::hz(value),
                    Q_BUTTERWORTH_F32,
                )
                .unwrap(),
            );
        });

        let switch2 = ctx.local.switch2;
        switch2.update();

        // switches are configured as active low
        if switch2.is_low() {
            ctx.local.led1.set_high().unwrap();
        }

        if switch2.is_high() {
            ctx.local.led1.set_low().unwrap();
        }

        let encoder = ctx.local.encoder;
        encoder.update();

        ctx.shared.encoder_value.lock(|encoder_value| {
            if encoder.current_value != *encoder_value {
                *encoder_value = encoder.current_value;
            }
        });
    }
}
