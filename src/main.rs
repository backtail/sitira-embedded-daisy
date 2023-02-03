#![no_main]
#![no_std]

pub mod binary_input;
pub mod dual_mux_4051;
pub mod encoder;
pub mod lcd;
pub mod rgbled;
// pub mod sd_card;
pub mod sitira;

pub const CONTROL_RATE_IN_MS: u32 = 30;
pub const LCD_REFRESH_RATE_IN_MS: u32 = 20;
pub const RECORD_SIZE: usize = 0x2000000;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{
        sitira::{AdcMuxInputs, AudioRate, ControlRate, Sitira, VisualRate},
        CONTROL_RATE_IN_MS, RECORD_SIZE,
    };
    use granulator::{Granulator, GranulatorParameter};
    use stm32h7xx_hal::prelude::_embedded_hal_adc_OneShot;

    use libdaisy::prelude::*;

    #[allow(unused_imports)]
    use crate::rprintln;

    #[shared]
    struct Shared {
        granulator: Granulator,

        sdram: &'static mut [f32],
        source_length: usize,

        is_recording: bool,
        is_playing: bool,
    }

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
        vr: VisualRate,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let sitira = Sitira::init(ctx.core, ctx.device);

        // create the granulator object
        let granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // set master volume to 1.0
        // granulator.set_parameter(GranulatorParameter::MasterVolume, 1.0);

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);

        (
            Shared {
                granulator,

                sdram: sitira.sdram,
                source_length: 0,

                is_recording: false,
                is_playing: true,
            },
            Local {
                ar: sitira.audio_rate,
                cr: sitira.control_rate,
                vr: sitira.visual_rate,
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
    #[task(binds = DMA1_STR1, local = [ar], shared = [granulator, sdram, source_length, is_playing, is_recording], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;

        let mut granulator = ctx.shared.granulator;
        let is_recording = ctx.shared.is_recording;
        let is_playing = ctx.shared.is_playing;

        let sdram = &mut ctx.shared.sdram;
        let source_length = &mut ctx.shared.source_length;

        audio.get_stereo(&mut buffer);

        (is_playing, is_recording).lock(|is_playing, is_recording| {
            // when recording
            if (!*is_playing && *is_recording) || (*is_playing && *is_recording) {
                (sdram, source_length).lock(|sdram, source_length| {
                    if *source_length < RECORD_SIZE {
                        for (index, (right, left)) in buffer.iter().enumerate() {
                            sdram[*source_length + index] = *right;
                            audio.push_stereo((*right, *left)).unwrap();
                        }
                        *source_length += buffer.len()
                    }
                });
            }
            // when playing
            if *is_playing && !*is_recording {
                granulator.lock(|granulator| {
                    for _ in buffer {
                        let mono_sample = granulator.get_next_sample();
                        audio.push_stereo((mono_sample, mono_sample)).unwrap();
                    }
                });
            }
        });
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [cr], shared = [granulator, sdram, source_length, is_recording], priority = 3)]
    fn update_handler(mut ctx: update_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.cr.timer2.clear_irq();

        // LEDs
        let led1 = &mut ctx.local.cr.led1;
        let led2 = &mut ctx.local.cr.led2;
        let led3 = &mut ctx.local.cr.led3;

        // binary devices
        let button = &mut ctx.local.cr.button;
        let gate1 = &mut ctx.local.cr.gate1;
        let gate2 = &mut ctx.local.cr.gate2;
        let gate3 = &mut ctx.local.cr.gate3;
        let gate4 = &mut ctx.local.cr.gate4;

        // adc related
        let adc_values = &mut ctx.local.cr.muxed_parameters;
        let adc2 = &mut ctx.local.cr.adc2;
        let master_volume = &mut ctx.local.cr.master_volume;

        // audio related
        let granulator = &mut ctx.shared.granulator;
        let sdram = &mut ctx.shared.sdram;
        let source_length = &mut ctx.shared.source_length;

        // save all binary inputs at the beginning
        button.save_state();
        gate1.save_state();
        gate2.save_state();
        gate3.save_state();
        gate4.save_state();

        if gate1.is_saved_state_high() || gate3.is_saved_state_high() {
            led1.set_high().unwrap();
        } else {
            led1.set_low().unwrap();
        }

        if gate2.is_saved_state_high() || gate4.is_saved_state_high() {
            led2.set_high().unwrap();
        } else {
            led2.set_low().unwrap();
        }

        let mut is_recording = false;
        ctx.shared.is_recording.lock(|value| {
            if button.is_triggered() {
                *value = !*value;
            }

            is_recording = *value;
        });

        match is_recording {
            true => {
                led3.set_high().unwrap();

                if button.is_triggered() {
                    rprintln!("Started recording incoming audio!");
                    granulator.lock(|granulator| granulator.remove_audio_buffer());
                    source_length.lock(|length| *length = 0);
                }
            }

            false => {
                led3.set_low().unwrap();

                if button.is_triggered() {
                    rprintln!("Stopped recording incoming audio!");
                    (sdram, source_length).lock(|sdram, source_length| {
                        let audio_buffer = &sdram[0..*source_length];
                        granulator.lock(|granulator| {
                            granulator.set_audio_buffer(audio_buffer);
                            rprintln!(
                                "Audio buffer gets set with length of {} samples!",
                                *source_length
                            );
                        });
                    });
                }
            }
        }

        // ----------------------------------
        // USER SETTINGS
        // ----------------------------------

        for i in 0..16 {
            adc_values.read_value(i);
        }

        if let Ok(data) = adc2.read(master_volume.get_pin()) {
            master_volume.update(data);
        }

        let parameter_array = [
            master_volume.get_value(),
            adc_values.get_value(AdcMuxInputs::ActiveGrains as usize),
            adc_values.get_value(AdcMuxInputs::Offset as usize),
            adc_values.get_value(AdcMuxInputs::GrainSize as usize),
            adc_values.get_value(AdcMuxInputs::Pitch as usize),
            adc_values.get_value(AdcMuxInputs::Delay as usize),
            adc_values.get_value(AdcMuxInputs::Velocity as usize),
            adc_values.get_value(AdcMuxInputs::OffsetSpread as usize),
            adc_values.get_value(AdcMuxInputs::GrainSizeSpread as usize),
            adc_values.get_value(AdcMuxInputs::PitchSpread as usize),
            adc_values.get_value(AdcMuxInputs::VelocitySpread as usize),
            adc_values.get_value(AdcMuxInputs::DelaySpread as usize),
        ];

        GranulatorParameter::update_all(parameter_array);

        granulator.lock(|g| {
            g.update_scheduler(core::time::Duration::from_millis(CONTROL_RATE_IN_MS as u64));
        });
    }

    #[task(binds = TIM4, local = [vr], shared = [])]
    fn display_handler(ctx: display_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.vr.timer4.clear_irq();

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);
    }
}
