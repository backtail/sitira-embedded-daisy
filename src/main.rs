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
        sdram,
        sitira::{AdcMuxInputs, AudioRate, ControlRate, Sitira, VisualRate},
    };

    use granulator::{Granulator, ModeType, ScaleType, UserSettings, WindowFunction};
    use stm32h7xx_hal::prelude::_embedded_hal_adc_OneShot;

    use libdaisy::prelude::OutputPin;

    use core::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    };

    #[allow(unused_imports)]
    use crate::rprintln;

    #[shared]
    struct Shared {
        audio_buffer: &'static [f32],
        user_settings: granulator::UserSettings,
    }

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
        vr: VisualRate,
        sdram: &'static mut [f32],
        granulator: Granulator,
    }

    static SOURCE_LENGTH: AtomicUsize = AtomicUsize::new(0);
    static IS_RECORDING: AtomicBool = AtomicBool::new(true);
    const AUDIO_CALLBACK_INTERVAL: f32 =
        libdaisy::AUDIO_BLOCK_SIZE as f32 * (1.0 / (libdaisy::AUDIO_SAMPLE_RATE as f32));

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let sitira = Sitira::init(ctx.core, ctx.device);

        // create the granulator object
        let granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);

        rprintln!("I am here!");

        (
            Shared {
                audio_buffer: sdram::get_slice(0, 1).unwrap(), // mock slice
                user_settings: UserSettings {
                    master_volume: 1.0,
                    active_grains: 0.1,
                    offset: 0.5,
                    grain_size: 0.5,
                    pitch: 0.5,
                    delay: 0.0,
                    velocity: 1.0,
                    sp_offset: 0.0,
                    sp_grain_size: 0.0,
                    sp_pitch: 0.0,
                    sp_delay: 0.0,
                    sp_velocity: 0.0,
                    window_function: WindowFunction::Sine as u8,
                    window_param: 0.5,
                    scale: ScaleType::Diatonic as u8,
                    mode: ModeType::Ionian as u8,
                },
            },
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
    #[task(binds = DMA1_STR1, local = [ar, sdram, granulator], shared = [user_settings, audio_buffer], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;
        let granulator = ctx.local.granulator;
        let sdram = ctx.local.sdram;

        audio.get_stereo(&mut buffer);

        // update scheduler
        granulator.update_scheduler(Duration::from_secs_f32(AUDIO_CALLBACK_INTERVAL));

        let is_recording = IS_RECORDING.load(Ordering::Relaxed);

        // when recording
        if is_recording {
            let source_length = SOURCE_LENGTH.load(Ordering::Relaxed);

            if source_length < sdram::SDRAM_SIZE {
                // store incomong audio in memory
                for (index, (right, left)) in buffer.iter().enumerate() {
                    sdram[source_length + index] = *right;
                    audio.push_stereo((*right, *left)).unwrap();
                }

                // update source length by buffer size of one channel
                SOURCE_LENGTH.fetch_add(buffer.len(), Ordering::Relaxed);
            } else {
                // wrap around the SDRAM when overflowing
                SOURCE_LENGTH.store(0, Ordering::Relaxed);

                // store incomong audio in memory
                for (index, (right, left)) in buffer.iter().enumerate() {
                    sdram[source_length + index] = *right;
                    audio.push_stereo((*right, *left)).unwrap();
                }
                SOURCE_LENGTH.fetch_add(buffer.len(), Ordering::Relaxed);
            }
        }

        // when playing
        if !is_recording {
            // set audio buffer
            let source_length = SOURCE_LENGTH.load(Ordering::Relaxed);
            granulator.set_audio_buffer(&sdram[0..source_length]);

            // update user settings
            ctx.shared
                .user_settings
                .lock(|settings| granulator.update_all_user_settings(settings));

            for _ in buffer {
                // get next sample
                let mono_sample = granulator.get_next_sample();
                audio.push_stereo((mono_sample, mono_sample)).unwrap();
            }
        }
    }

    #[task(binds = TIM2, local = [cr], shared = [user_settings], priority = 3)]
    fn update_handler(mut ctx: update_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.cr.timer2.clear_irq();

        // ----------------------------------
        // BUTTON, GATE INs AND LEDs
        // ----------------------------------

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
                if button.is_triggered() {
                    rprintln!("Started recording incoming audio!");
                    SOURCE_LENGTH.store(0, Ordering::Relaxed);
                }

                led3.set_high().unwrap();
            }

            false => {
                if button.is_triggered() {
                    rprintln!("Stopped recording incoming audio!");
                    rprintln!(
                        "Audio buffer gets set with length of {} samples!",
                        SOURCE_LENGTH.load(Ordering::Relaxed)
                    );
                }

                led3.set_low().unwrap();
            }
        }

        // can probably be spilt into two different task, since reading the ADCs needs more fine tuning

        // ----------------------------------
        // USER SETTINGS
        // ----------------------------------

        let adc_values = &mut ctx.local.cr.muxed_parameters;
        let adc2 = &mut ctx.local.cr.adc2;
        let master_volume = &mut ctx.local.cr.master_volume;

        // read from ADC2
        for i in 0..16 {
            adc_values.read_value(i);
        }

        // read from ADC1
        if let Ok(data) = adc2.read(master_volume.get_pin()) {
            master_volume.update(data);
        }

        let window_function = (adc_values.get_value(AdcMuxInputs::Envelope as usize) * 6.0) as u8;
        rprintln!("Window Function: {}", window_function);
        // update user settings
        ctx.shared.user_settings.lock(|settings| {
            settings.master_volume = master_volume.get_value();
            settings.active_grains = adc_values.get_value(AdcMuxInputs::ActiveGrains as usize);
            settings.offset = adc_values.get_value(AdcMuxInputs::Offset as usize);
            settings.grain_size = adc_values.get_value(AdcMuxInputs::GrainSize as usize);
            settings.pitch = adc_values.get_value(AdcMuxInputs::Pitch as usize);
            settings.delay = adc_values.get_value(AdcMuxInputs::Delay as usize);
            settings.velocity = adc_values.get_value(AdcMuxInputs::Velocity as usize);
            settings.sp_offset = adc_values.get_value(AdcMuxInputs::OffsetSpread as usize);
            settings.sp_grain_size = adc_values.get_value(AdcMuxInputs::GrainSizeSpread as usize);
            settings.sp_pitch = adc_values.get_value(AdcMuxInputs::PitchSpread as usize);
            settings.sp_velocity = adc_values.get_value(AdcMuxInputs::VelocitySpread as usize);
            settings.sp_delay = adc_values.get_value(AdcMuxInputs::DelaySpread as usize);
            settings.window_function = window_function;
            // settings.window_param = adc_values.get_value(AdcMuxInputs::WaveSelect as usize);
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
