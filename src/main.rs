#![no_main]
#![no_std]

pub mod encoder;
pub mod lcd;
pub mod rgbled;
pub mod sitira;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::sitira::{AudioRate, ControlRate, Sitira};
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

        // get all hardware
        let adc1 = &mut ctx.local.cr.adc1;
        let pot1 = &mut ctx.local.cr.pot1;
        let pot2 = &mut ctx.local.cr.pot2;
        let switch1 = &mut ctx.local.cr.switch1;
        let switch2 = &mut ctx.local.cr.switch2;
        let led1 = &mut ctx.local.cr.led1;
        let led2 = &mut ctx.local.cr.led2;
        let encoder = &mut ctx.local.cr.encoder;

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

        // cycle switch color
        if switch1.is_pressed() {
            led1.cycle_color();
        }
        if switch2.is_pressed() {
            led2.cycle_color();
        }

        // calculate buffer offset
        let offset = (ctx.local.cr.file_length_in_samples as f32 * pot1.get_value()) as usize;

        // update the granulator with the new values
        ctx.shared.granulator.lock(|granulator| {
            granulator.set_offset(offset);
            granulator.set_active_grains(granulator::MAX_GRAINS);
            granulator.set_grain_size(encoder.current_value as f32 * 20.0);
            granulator.set_pitch(pot2.get_value() * 5.0);
            granulator.set_master_volume(1.0);

            granulator.update_scheduler(core::time::Duration::from_millis(1));
        });
    }
}
