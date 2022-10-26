#![no_main]
#![no_std]

pub mod binary_input;
pub mod dual_mux_4051;
pub mod encoder;
pub mod lcd;
pub mod rgbled;
pub mod sd_card;
pub mod sitira;

pub const CONTROL_RATE_IN_MS: u32 = 50;
pub const LCD_REFRESH_RATE_IN_MS: u32 = 20;
pub const RECORD_SIZE: usize = 0x2000000;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{
        sitira::{AdcMuxInputs, AudioRate, ControlRate, Sitira, VisualRate},
        CONTROL_RATE_IN_MS,
    };
    use granulator::{Granulator, GranulatorParameter};

    use libdaisy::prelude::*;

    #[cfg(feature = "log")]
    use rtt_target::rprintln;

    #[shared]
    struct Shared {
        granulator: Granulator,
        sdram: &'static mut [f32],
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

        libdaisy::logger::init();

        // init logging via RTT
        #[cfg(feature = "log")]
        {
            rprintln!("RTT loggging initiated!");
        }
        // create the granulator object
        let mut granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // set master volume to 1.0
        // granulator.set_master_volume(1.0);
        granulator.set_parameter(GranulatorParameter::MasterVolume, 1.0);

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);

        (
            Shared {
                granulator,
                sdram: sitira.sdram,
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
    #[task(binds = DMA1_STR1, local = [ar], shared = [granulator], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;

        let mut granulator = ctx.shared.granulator;

        audio.get_stereo(&mut buffer);

        granulator.lock(|granulator| {
            for _ in buffer {
                let mono_sample = granulator.get_next_sample();
                audio.push_stereo((mono_sample, mono_sample)).unwrap();
            }
        });
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [cr], shared = [granulator])]
    fn update_handler(mut ctx: update_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.cr.timer2.clear_irq();

        let gate1 = &mut ctx.local.cr.gate1;
        let gate2 = &mut ctx.local.cr.gate2;
        let gate3 = &mut ctx.local.cr.gate3;
        let gate4 = &mut ctx.local.cr.gate4;
        let led1 = &mut ctx.local.cr.led1;
        let led2 = &mut ctx.local.cr.led2;

        if gate1.is_high() || gate3.is_high() {
            led1.set_high().unwrap();
        } else {
            led1.set_low().unwrap();
        }

        if gate2.is_high() || gate4.is_high() {
            led2.set_high().unwrap();
        } else {
            led2.set_low().unwrap();
        }

        let button = &mut ctx.local.cr.button;
        let led3 = &mut ctx.local.cr.led3;

        if button.is_high() {
            led3.set_high().unwrap();
        } else {
            led3.set_low().unwrap();
        }

        let granulator = &mut ctx.shared.granulator;

        let adc_values = &mut ctx.local.cr.muxed_parameters;
        for i in 0..16 {
            adc_values.read_value(i);
        }

        #[cfg(feature = "log")]
        {
            rprintln!("ADC{}: {:.4} ", 0, adc_values.get_value(0));
        }

        // update the scheduler
        granulator.lock(|g| {
            g.set_parameter(
                GranulatorParameter::ActiveGrains,
                adc_values.get_value(AdcMuxInputs::ActiveGrains as usize),
            );
            g.set_parameter(
                GranulatorParameter::Delay,
                adc_values.get_value(AdcMuxInputs::Delay as usize),
            );
            g.set_parameter(
                GranulatorParameter::DelaySpread,
                adc_values.get_value(AdcMuxInputs::DelaySpread as usize),
            );
            g.set_parameter(
                GranulatorParameter::GrainSize,
                adc_values.get_value(AdcMuxInputs::GrainSize as usize),
            );
            g.set_parameter(
                GranulatorParameter::GrainSizeSpread,
                adc_values.get_value(AdcMuxInputs::GrainSizeSpread as usize),
            );
            // g.set_parameter(
            //     GranulatorParameter::MasterVolume,
            //     MasterVolume,
            // );
            g.set_parameter(
                GranulatorParameter::Offset,
                adc_values.get_value(AdcMuxInputs::Offset as usize),
            );
            g.set_parameter(
                GranulatorParameter::OffsetSpread,
                adc_values.get_value(AdcMuxInputs::OffsetSpread as usize),
            );
            g.set_parameter(
                GranulatorParameter::Pitch,
                adc_values.get_value(AdcMuxInputs::Pitch as usize),
            );
            g.set_parameter(
                GranulatorParameter::PitchSpread,
                adc_values.get_value(AdcMuxInputs::PitchSpread as usize),
            );
            g.set_parameter(
                GranulatorParameter::Velocity,
                adc_values.get_value(AdcMuxInputs::Velocity as usize),
            );
            g.set_parameter(
                GranulatorParameter::VelocitySpread,
                adc_values.get_value(AdcMuxInputs::VelocitySpread as usize),
            );

            // update scheduler
            g.update_scheduler(core::time::Duration::from_millis(CONTROL_RATE_IN_MS as u64));
        });
    }

    #[task(binds = TIM4, local = [vr], shared = [sdram])]
    fn display_handler(ctx: display_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.vr.timer4.clear_irq();

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);
    }
}
