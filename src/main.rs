#![no_main]
#![no_std]

pub mod encoder;
pub mod lcd;
pub mod sitira;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use granulator::Granulator;
    use sitira::{AudioRate, ControlRate, Sitira};

    use libdaisy::prelude::*;

    use crate::sitira;

    #[shared]
    struct Shared {
        granulator: Granulator,
    }

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
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
    #[task(binds = TIM2, local = [cr], shared = [granulator])]
    fn update_handler(mut ctx: update_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.cr.timer2.clear_irq();

        // update adc
        let adc1 = &mut ctx.local.cr.adc1;
        let control2 = &mut ctx.local.cr.control2;
        if let Ok(data) = adc1.read(control2.get_pin()) {
            control2.update(data);
        }

        // update switch
        let switch2 = &mut ctx.local.cr.switch2;
        switch2.update();

        // switches are configured as active low
        if switch2.is_low() {
            ctx.local.cr.led1.set_high().unwrap();
        }
        if switch2.is_high() {
            ctx.local.cr.led1.set_low().unwrap();
        }

        // update encoder
        let encoder = &mut ctx.local.cr.encoder;
        encoder.update();

        // calculate buffer offset
        let offset = (ctx.local.cr.file_length_in_samples as f32 * control2.get_value()) as usize;

        // update the granulator with the new values
        ctx.shared.granulator.lock(|granulator| {
            granulator.set_offset(offset);
            granulator.set_active_grains(granulator::MAX_GRAINS / 2);
            granulator.set_grain_size(300.0);
            granulator.set_pitch(encoder.current_value as f32);
            granulator.set_master_volume(5.0);
        });
    }
}
