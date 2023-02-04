#![no_main]
#![no_std]

pub mod binary_input;
pub mod config;
pub mod dual_mux_4051;
pub mod encoder;
pub mod lcd;
pub mod rgbled;
pub mod sdram;
pub mod sitira;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{
        config::*,
        sdram,
        sitira::{AdcMuxInputs, AudioRate, ControlRate, Sitira, VisualRate},
    };

    use granulator::{Granulator, GranulatorParameter};
    use stm32h7xx_hal::prelude::_embedded_hal_adc_OneShot;

    use libdaisy::prelude::OutputPin;

    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[allow(unused_imports)]
    use crate::rprintln;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
        vr: VisualRate,
        sdram: &'static mut [f32],
        granulator: Granulator,
    }

    static SOURCE_LENGTH: AtomicUsize = AtomicUsize::new(0);
    static IS_RECORDING: AtomicBool = AtomicBool::new(false);

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let sitira = Sitira::init(ctx.core, ctx.device);

        // create the granulator object
        let granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);

        (
            Shared {},
            Local {
                ar: sitira.audio_rate,
                cr: sitira.control_rate,
                vr: sitira.visual_rate,
                sdram: sitira.sdram,
                granulator,
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
    #[task(binds = DMA1_STR1, local = [ar, sdram], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;

        audio.get_stereo(&mut buffer);

        let is_recording = IS_RECORDING.load(Ordering::Relaxed);

        // when recording
        if is_recording {
            let sdram = ctx.local.sdram;
            let source_length = SOURCE_LENGTH.load(Ordering::Relaxed);

            if source_length < sdram::SDRAM_SIZE {
                for (index, (right, left)) in buffer.iter().enumerate() {
                    sdram[source_length + index] = *right;
                    audio.push_stereo((*right, *left)).unwrap();
                }
                SOURCE_LENGTH.fetch_add(buffer.len(), Ordering::Relaxed);
            }
        }

        // when playing
        if !is_recording {
            for _ in buffer {
                let mono_sample = granulator::get_next_sample();
                audio.push_stereo((mono_sample, mono_sample)).unwrap();
            }
        }
    }

    #[task(binds = TIM2, local = [cr, granulator], priority = 3)]
    fn update_handler(ctx: update_handler::Context) {
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
        let granulator = ctx.local.granulator;

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

        if button.is_triggered() {
            IS_RECORDING.fetch_xor(true, Ordering::Relaxed); // invert boolean
        }

        match IS_RECORDING.load(Ordering::Relaxed) {
            true => {
                led3.set_high().unwrap();

                if button.is_triggered() {
                    rprintln!("Started recording incoming audio!");
                    granulator.remove_audio_buffer();
                    SOURCE_LENGTH.store(0, Ordering::Relaxed);
                }
            }

            false => {
                led3.set_low().unwrap();

                if button.is_triggered() {
                    rprintln!("Stopped recording incoming audio!");

                    if let Some(audio_buffer) =
                        sdram::get_slice::<f32>(0, SOURCE_LENGTH.load(Ordering::Relaxed))
                    {
                        granulator.set_audio_buffer(audio_buffer);
                        rprintln!(
                            "Audio buffer gets set with length of {} samples!",
                            SOURCE_LENGTH.load(Ordering::Relaxed)
                        );
                    } else {
                        rprintln!("Audio buffer doesn't fit into SDRAM, abort sample loading!");
                    }
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

        granulator.update_scheduler(core::time::Duration::from_millis(CONTROL_RATE_IN_MS as u64));
    }

    #[task(binds = TIM4, local = [vr], shared = [])]
    fn display_handler(ctx: display_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.vr.timer4.clear_irq();

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);
    }
}
