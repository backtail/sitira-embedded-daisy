#![no_main]
#![no_std]

pub mod encoder;
pub mod granular;
pub mod lcd;
pub mod sitira;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{encoder, granular, sitira};
    use biquad::*;
    use libdaisy::{audio, gpio::*, hid, prelude::*};
    use stm32h7xx_hal::timer::Timer;
    use stm32h7xx_hal::{adc, stm32};

    #[shared]
    struct Shared {
        pot2_value: f32,
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
        grains: granular::Grains,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let s = sitira::Sitira::init(ctx.core, ctx.device);
        let mut g = granulator::Granulator::new();

        g.get_next_sample();
        g.update_scheduler(core::time::Duration::from_millis(10));
        g.set_active_grains(1);
        let buffer = [0_f32; 64];
        g.set_audio_buffer(&buffer[..]);
        g.set_grain_size(100.0);
        g.set_master_volume(0.5);
        g.set_offset(1000);

        (
            Shared {
                pot2_value: 0.0,
                encoder_value: s.encoder_value,
                biquad: s.biquad,
            },
            Local {
                audio: s.audio,
                buffer: s.buffer,
                sdram: s.sdram,
                playhead: s.playhead,
                file_length_in_samples: s.file_length_in_samples,
                adc1: s.adc1,
                control2: s.control2,
                timer2: s.timer2,
                led1: s.led1,
                switch2: s.switch2,
                encoder: s.encoder,
                grains: s.grains,
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
    #[task(binds = DMA1_STR1, local = [audio, buffer, playhead, sdram, grains], shared = [pot2_value, biquad], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let mut buffer = *ctx.local.buffer;
        let sdram: &mut [f32] = *ctx.local.sdram;
        let grains = ctx.local.grains;
        let mut pot2_value = ctx.shared.pot2_value;

        pot2_value.lock(|pot2_value| {
            grains.set_offset(*pot2_value as u32);
            grains.start_window_funtion();
        });

        audio.get_stereo(&mut buffer);
        for (_left, _right) in buffer {
            let mut mono_sum = 0.0_f32;

            for instance in 0..grains.active_grains {
                let mut current_grain = grains.grains[instance];

                if !current_grain.done_with_window_funtion {
                    let position = current_grain.update_sample_position();
                    mono_sum += current_grain.update_next_sample(sdram[position]);
                }
            }
            mono_sum = mono_sum / grains.active_grains as f32;

            audio.push_stereo((mono_sum, mono_sum)).unwrap();
        }
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [timer2, adc1, control2, switch2, led1, encoder, file_length_in_samples], shared = [encoder_value, pot2_value])]
    fn interface_handler(mut ctx: interface_handler::Context) {
        ctx.local.timer2.clear_irq();
        let adc1 = ctx.local.adc1;
        let control2 = ctx.local.control2;

        if let Ok(data) = adc1.read(control2.get_pin()) {
            control2.update(data);
        }

        ctx.shared.pot2_value.lock(|pot2_value| {
            *pot2_value = control2.get_value() * (*ctx.local.file_length_in_samples) as f32
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
