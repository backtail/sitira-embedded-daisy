#![no_main]
#![no_std]

pub mod encoder;
pub mod lcd;
pub mod rgbled;
pub mod sitira;

pub const CONTROL_RATE_IN_MS: u32 = 10;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{
        rgbled::RGBColors,
        sitira::{AudioRate, ControlRate, Sitira},
        CONTROL_RATE_IN_MS,
    };
    use granulator::Granulator;
    use libdaisy::prelude::*;

    #[shared]
    struct Shared {
        granulator: Granulator,
    }

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
        parameter_page: usize,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let sitira = Sitira::init(ctx.core, ctx.device);

        // create the granulator object
        let mut granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // create slice of loaded audio files
        let slice = &sitira.control_rate.sdram[0..sitira.control_rate.file_length_in_samples];

        // set the audio buffer
        granulator.set_audio_buffer(slice);

        (
            Shared { granulator },
            Local {
                ar: sitira.audio_rate,
                cr: sitira.control_rate,
                parameter_page: 0,
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
    #[task(binds = DMA1_STR1, local = [ar], shared = [granulator], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;
        let mut granulator = ctx.shared.granulator;

        // is somehow necessary
        audio.get_stereo(&mut buffer);

        // loop over buffer
        for (_left, _right) in buffer {
            let mut mono_sample: f32 = 0.0;

            // lock granulator
            granulator.lock(|granulator| {
                mono_sample = granulator.get_next_sample();
            });

            // push audio into stream
            audio.push_stereo((mono_sample, mono_sample)).unwrap();
        }
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [cr, parameter_page], shared = [granulator])]
    fn update_handler(ctx: update_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.cr.timer2.clear_irq();

        // get all hardware
        let adc1 = &mut ctx.local.cr.adc1;
        let pot1 = &mut ctx.local.cr.pot1;
        let pot2 = &mut ctx.local.cr.pot2;
        let switch1 = &mut ctx.local.cr.switch1;
        let switch2 = &mut ctx.local.cr.switch2;
        let led1 = &mut ctx.local.cr.led1;
        let led2 = &mut ctx.local.cr.led2;
        let encoder = &mut ctx.local.cr.encoder;

        // local parameters
        let mut parameter_page = *ctx.local.parameter_page;

        // shared
        let mut granulator = ctx.shared.granulator;

        // update all the hardware
        if let Ok(data) = adc1.read(pot1.get_pin()) {
            pot1.update(data);
        }
        if let Ok(data) = adc1.read(pot2.get_pin()) {
            pot2.update(data);
        }
        switch1.update();
        switch2.update();
        led1.update();
        led2.update();
        encoder.update();

        // detect if encoder button has been pressed
        // if pressed, change parameter page
        if encoder.switch.is_pressed() {
            parameter_page += 1;
            if parameter_page > 3 {
                parameter_page = 0;
            }
        }

        // set parameter depending on current page
        match parameter_page {
            // Grain Size
            0 => {
                led2.set_simple_color(RGBColors::Blue);
                granulator.lock(|g| {
                    g.set_grain_size(pot1.get_value() * ctx.local.cr.file_length_in_samples as f32);
                });
            }

            // Pitch
            1 => {
                led2.set_simple_color(RGBColors::Red);
                granulator.lock(|g| {
                    g.set_pitch(pot1.get_value() * 20.0);
                });
            }

            // Offset
            2 => {
                led2.set_simple_color(RGBColors::Green);
                granulator.lock(|g| {
                    g.set_offset(
                        (pot1.get_value() * ctx.local.cr.file_length_in_samples as f32) as usize,
                    );
                });
            }

            // Active Grains
            3 => {
                led2.set_simple_color(RGBColors::White);
                granulator.lock(|g| {
                    g.set_active_grains(
                        (pot1.get_value() * granulator::MAX_GRAINS as f32) as usize,
                    );
                });
            }

            _ => {}
        }

        // set master volume
        granulator.lock(|g| {
            g.set_master_volume(encoder.current_value as f32 * 0.5);
        });

        // update the scheduler
        granulator.lock(|g| {
            g.update_scheduler(core::time::Duration::from_millis(CONTROL_RATE_IN_MS as u64));
        });
    }
}
