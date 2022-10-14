#![no_main]
#![no_std]

pub mod encoder;
pub mod lcd;
pub mod rgbled;
pub mod sd_card;
pub mod sitira;

pub const CONTROL_RATE_IN_MS: u32 = 10;
pub const LCD_REFRESH_RATE_IN_MS: u32 = 50;
pub const RECORD_SIZE: usize = 0x2000000;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use crate::{
        rgbled::RGBColors,
        sitira::{AudioRate, ControlRate, Sitira, VisualRate},
        CONTROL_RATE_IN_MS, RECORD_SIZE,
    };
    use granulator::{Granulator, GranulatorParameter::*};
    use libdaisy::prelude::*;

    #[cfg(feature = "log")]
    use rtt_target::rprintln;

    #[shared]
    struct Shared {
        granulator: Granulator,
        recording_state_switched: bool,
        is_recording: bool,
        sdram: &'static mut [f32],
        source_length: usize,
    }

    #[local]
    struct Local {
        ar: AudioRate,
        cr: ControlRate,
        vr: VisualRate,
        parameter_page: usize,
        shift: bool,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        // initiate system
        let sitira = Sitira::init(ctx.core, ctx.device);

        libdaisy::logger::init();

        // init logging via RTT
        #[cfg(feature = "log")]
        {
            rprintln!("LOL");
        }
        // create the granulator object
        let mut granulator = Granulator::new(libdaisy::AUDIO_SAMPLE_RATE);

        // set master volume to 1.0
        // granulator.set_master_volume(1.0);
        granulator.set_parameter(MasterVolume, 1.0);

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);

        (
            Shared {
                granulator,
                recording_state_switched: true,
                is_recording: false,
                sdram: sitira.sdram,
                source_length: 0,
            },
            Local {
                ar: sitira.audio_rate,
                cr: sitira.control_rate,
                vr: sitira.visual_rate,
                parameter_page: 0,
                shift: false,
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
    #[task(binds = DMA1_STR1, local = [ar], shared = [granulator, sdram, is_recording, source_length], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = &mut ctx.local.ar.audio;
        let mut buffer = ctx.local.ar.buffer;

        let mut granulator = ctx.shared.granulator;
        let mut sdram = ctx.shared.sdram;
        let mut is_recording = false;
        ctx.shared.is_recording.lock(|f| is_recording = *f);
        let mut source_length = 0;
        ctx.shared.source_length.lock(|f| source_length = *f);

        audio.get_stereo(&mut buffer);

        if is_recording && source_length < RECORD_SIZE {
            sdram.lock(|sdram| {
                for (index, (right, left)) in buffer.iter().enumerate() {
                    sdram[source_length + index] = (right + left) * 0.5;
                    audio.push_stereo((*right, *left)).unwrap();
                }
            });

            ctx.shared
                .source_length
                .lock(|length| *length += buffer.len());
        }

        if !is_recording {
            granulator.lock(|granulator| {
                for _ in buffer {
                    let mono_sample = granulator.get_next_sample();
                    audio.push_stereo((mono_sample, mono_sample)).unwrap();
                }
            });
        }
    }

    // read values from pot 2 and switch 2 of daisy pod
    #[task(binds = TIM2, local = [cr, parameter_page, shift], shared = [granulator, recording_state_switched, is_recording])]
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

        // local parameters
        let parameter_page = ctx.local.parameter_page;
        let shift = ctx.local.shift;

        // shared
        let mut granulator = ctx.shared.granulator;
        let recording_state_switched = &mut ctx.shared.recording_state_switched;
        let is_recording = &mut ctx.shared.is_recording;

        // update all the hardware
        if let Ok(data) = adc1.read(pot1.get_pin()) {
            pot1.update(data);
        }
        if let Ok(data) = adc1.read(pot2.get_pin()) {
            pot2.update(data);
        }
        led1.update();
        led2.update();
        switch1.update();
        switch2.update();
        encoder.update();

        // parameter pages
        if switch2.is_held() {
            *parameter_page += 1;
            if *parameter_page > 4 {
                *parameter_page = 0;
            }
        }

        if *parameter_page == 0 {
            led2.set_simple_color(RGBColors::Blue);
            granulator.lock(|g| {
                g.set_parameter(GrainSize, pot1.get_value());
                g.set_parameter(GrainSizeSpread, pot2.get_value());
            });
        }
        if *parameter_page == 1 {
            led2.set_simple_color(RGBColors::Green);
            granulator.lock(|g| {
                g.set_parameter(Offset, pot1.get_value());
                g.set_parameter(OffsetSpread, pot2.get_value());
            });
        }
        if *parameter_page == 2 {
            led2.set_simple_color(RGBColors::Magenta);
            granulator.lock(|g| {
                g.set_parameter(Pitch, pot1.get_value());
                g.set_parameter(PitchSpread, pot2.get_value());
            });
        }
        if *parameter_page == 3 {
            led2.set_simple_color(RGBColors::Cyan);
            granulator.lock(|g| {
                g.set_parameter(Delay, pot1.get_value());
                g.set_parameter(DelaySpread, pot2.get_value());
            });
        }
        if *parameter_page == 4 {
            led2.set_simple_color(RGBColors::White);
            granulator.lock(|g| {
                g.set_parameter(ActiveGrains, pot1.get_value());
                g.set_parameter(MasterVolume, pot2.get_value());
            });
        }

        // shift button
        if switch1.is_held() {
            *shift = !*shift;
            recording_state_switched.lock(|f| *f = true);
        } else {
            recording_state_switched.lock(|f| *f = false);
        }

        if *shift {
            led1.set_simple_color(RGBColors::Red);
            is_recording.lock(|f| *f = true);
        } else {
            led1.set_simple_color(RGBColors::Black);
            is_recording.lock(|f| *f = false);
        }

        // update the scheduler
        granulator.lock(|g| {
            g.update_scheduler(core::time::Duration::from_millis(CONTROL_RATE_IN_MS as u64));
        });
    }

    #[task(binds = TIM4, local = [vr], shared = [sdram, source_length, recording_state_switched, is_recording, granulator])]
    fn display_handler(mut ctx: display_handler::Context) {
        // clear TIM2 interrupt flag
        ctx.local.vr.timer4.clear_irq();

        // shared
        let mut recording_state_switched = false;
        ctx.shared
            .recording_state_switched
            .lock(|f| recording_state_switched = *f);
        let mut is_recording = false;
        ctx.shared.is_recording.lock(|f| is_recording = *f);

        let mut source_length = 0;
        ctx.shared.source_length.lock(|f| source_length = *f);

        let mut sdram = ctx.shared.sdram;

        // setup
        let lcd = &mut ctx.local.vr.lcd;

        if recording_state_switched {
            use embedded_graphics::geometry::Point;
            use embedded_graphics::pixelcolor::Rgb565;
            use embedded_graphics::prelude::*;
            if is_recording {
                sdram.lock(|sdram| sdram.fill(0.0));
                ctx.shared.source_length.lock(|length| *length = 0);
                lcd.fill_subsection_with_corners(
                    Point { x: 0, y: 0 },
                    Point { x: 61, y: 7 },
                    Rgb565::BLACK,
                );
                lcd.fill_subsection_with_corners(
                    Point { x: 62, y: 0 },
                    Point { x: 82, y: 7 },
                    Rgb565::CSS_RED,
                );
                lcd.print_on_screen(0, 5, "recording: on");
            } else {
                lcd.fill_subsection_with_corners(
                    Point { x: 0, y: 0 },
                    Point { x: 61, y: 7 },
                    Rgb565::BLACK,
                );
                lcd.fill_subsection_with_corners(
                    Point { x: 61, y: 0 },
                    Point { x: 87, y: 7 },
                    Rgb565::BLACK,
                );
                lcd.print_on_screen(0, 5, "recording: off");

                sdram.lock(|sdram| {
                    let audio_buffer = &sdram[0..source_length];
                    lcd.fill_subsection_with_corners(
                        Point { x: 0, y: 60 },
                        Point { x: 320, y: 180 },
                        Rgb565::BLACK,
                    );
                    lcd.draw_waveform(audio_buffer);
                    ctx.shared.granulator.lock(|g| {
                        g.set_audio_buffer(audio_buffer);
                    });
                });
            }
        }

        // activate timer 4 interrupt
        rtic::pend(stm32h7xx_hal::interrupt::TIM4);
    }
}
