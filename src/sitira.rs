use libdaisy::prelude::*;
use libdaisy::{audio, gpio::*, hid, sdmmc, system::System};

use stm32h7xx_hal::gpio::Floating;
use stm32h7xx_hal::spi::{Enabled, Mode, Spi};
use stm32h7xx_hal::stm32::SPI1;
use stm32h7xx_hal::time::U32Ext;
use stm32h7xx_hal::timer::Timer;
use stm32h7xx_hal::{adc, pac, stm32};

use crate::binary_input::*;
use crate::dual_mux_4051;
use crate::encoder;
use crate::lcd;
// use crate::sd_card::{self, SdCard};
use crate::{CONTROL_RATE_IN_MS, LCD_REFRESH_RATE_IN_MS};

// ===================
// PIN TYPE DEFINITION
// ===================

/// Not multiplexed
pub type MasterVolume = hid::AnalogControl<Daisy21<Analog>>;
/// MUX A+B
pub type MuxInput1 = Daisy15<Analog>;
/// MUX C+D
pub type MuxInput2 = Daisy16<Analog>;

pub type MuxSelect0 = Daisy17<Output<PushPull>>;
pub type MuxSelect1 = Daisy18<Output<PushPull>>;
pub type MuxSelect2 = Daisy19<Output<PushPull>>;

pub type AnalogRead =
    dual_mux_4051::DualMux<MuxInput1, MuxInput2, MuxSelect0, MuxSelect1, MuxSelect2>;

pub type Gate1 = BinaryInput<Daisy24<Input<Floating>>>;
pub type Gate2 = BinaryInput<Daisy25<Input<Floating>>>;
pub type Gate3 = BinaryInput<Daisy22<Input<Floating>>>;
pub type Gate4 = BinaryInput<Daisy23<Input<Floating>>>;

pub type KillGate = BinaryInput<Daisy20<Input<Floating>>>;

pub type Led1 = Daisy13<Output<PushPull>>;
pub type Led2 = Daisy14<Output<PushPull>>;
pub type Led3 = Daisy0<Output<PushPull>>;

pub type ButtonSwitch = BinaryInput<Daisy9<Input<PullDown>>>;

pub type Encoder = encoder::RotaryEncoder<
    Daisy28<Input<Floating>>,
    Daisy26<Input<PullUp>>,
    Daisy27<Input<PullUp>>,
>;

pub type Display = lcd::Lcd<
    Spi<SPI1, Enabled>,
    Daisy11<Output<PushPull>>,
    Daisy12<Output<PushPull>>,
    Daisy7<Output<PushPull>>,
>;

pub enum AdcMuxInputs {
    Offset = 0,
    GrainSize = 1,
    Pitch = 2,
    PitchSpread = 4,
    OffsetSpread = 5,
    GrainSizeSpread = 7,
    Delay = 8,
    ActiveGrains = 9,
    Envelope = 10,
    Velocity = 12,
    DelaySpread = 13,
    WaveSelect = 14,
    VelocitySpread = 15,
}

pub struct AudioRate {
    pub audio: audio::Audio,
    pub buffer: audio::AudioBuffer,
}

pub struct ControlRate {
    // HAL
    pub timer2: Timer<stm32::TIM2>,

    // Analog inputs
    pub master_volume: MasterVolume,
    pub muxed_parameters: AnalogRead,

    // Gates
    pub gate1: Gate1,
    pub gate2: Gate2,
    pub gate3: Gate3,
    pub gate4: Gate4,
    pub kill_gate: KillGate,

    // LEDs
    pub led1: Led1,
    pub led2: Led2,
    pub led3: Led3,
    pub seed_led: SeedLed,

    // Switches
    pub button: ButtonSwitch,
    pub encoder: Encoder,
}

pub struct VisualRate {
    pub lcd: Display,
    pub timer4: Timer<stm32::TIM4>,
}

pub struct Sitira {
    pub audio_rate: AudioRate,
    pub control_rate: ControlRate,
    pub visual_rate: VisualRate,
    pub sdram: &'static mut [f32],
    // pub sd_card: SdCard,
}

impl Sitira {
    /**
    Initializes the Daisy Seed for the Sitira platform. Automatically sets up all necessary peripherals:
    - SAI1/I²C (I²S Audio Codec/Configuration)
    - FMC (SDRAM Controller)
    - TIM2/TIM3/TIM4 (Internal Timing for Interrupts)
    - ADC1 (Analog Input Reading)
    - SPI1 (LCD Driver)
    - SDMMC1 (SD Card Controller)
    */
    pub fn init(core: rtic::export::Peripherals, device: stm32::Peripherals) -> Self {
        // ===========
        // SYSTEM INIT
        // ===========

        let mut system = System::init(core, device);

        let rcc_p = unsafe { pac::Peripherals::steal().RCC };
        let pwr_p = unsafe { pac::Peripherals::steal().PWR };
        let syscfg_p = unsafe { pac::Peripherals::steal().SYSCFG };

        let mut ccdr = System::init_clocks(pwr_p, rcc_p, &syscfg_p);

        // set high for system config
        let mut seed_led = system.gpio.led;
        seed_led.set_high().unwrap();

        // ============
        // CONFIG SDRAM
        // ============

        let sdram = system.sdram;
        sdram.fill(0.0);

        // =========================
        // CONFIG SD CARD CONNECTION
        // =========================

        // setting up SD card connection
        let sdmmc_d = unsafe { pac::Peripherals::steal().SDMMC1 };
        let _sd = sdmmc::init(
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

        // let sd_card = sd_card::SdCard::new(sd);

        // =============
        // CONFIG TIMERS
        // =============

        system.timer2.set_freq(CONTROL_RATE_IN_MS.ms());

        // Delay Timer
        let timer3 = unsafe { pac::Peripherals::steal().TIM3 }.timer(
            1.ms(),
            ccdr.peripheral.TIM3,
            &ccdr.clocks,
        );
        let delay = stm32h7xx_hal::delay::DelayFromCountDownTimer::new(timer3);

        let timer4_p = unsafe { pac::Peripherals::steal().TIM4 };
        let mut timer4 =
            stm32h7xx_hal::timer::Timer::tim4(timer4_p, ccdr.peripheral.TIM4, &mut ccdr.clocks);

        timer4.set_freq(LCD_REFRESH_RATE_IN_MS.ms());
        timer4.listen(stm32h7xx_hal::timer::Event::TimeOut);

        // ===========================
        // CONFIG LCD DRIVER (ILI9431)
        // ===========================

        let lcd_clk = system
            .gpio
            .daisy8
            .expect("Failed to get pin 8 of the daisy!")
            .into_alternate_af5();

        let lcd_miso = stm32h7xx_hal::spi::NoMiso {};

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

        // dummy pin
        let lcd_reset = system
            .gpio
            .daisy7
            .expect("Failed to get pin 7 of the daisy!")
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

        let mut lcd = lcd::Lcd::new(lcd_spi, lcd_dc, lcd_cs, lcd_reset, delay);

        lcd.setup();

        // =====================
        // CONFIG ANALOG READING
        // =====================

        let mux1_pin = system
            .gpio
            .daisy15
            .take()
            .expect("Failed to get pin 15 of the daisy!")
            .into_analog();

        let mux2_pin = system
            .gpio
            .daisy16
            .take()
            .expect("Failed to get pin 16 of the daisy!")
            .into_analog();

        let select0_pin = system
            .gpio
            .daisy17
            .take()
            .expect("Failed to get pin 17 of the daisy!")
            .into_push_pull_output();

        let select1_pin = system
            .gpio
            .daisy18
            .take()
            .expect("Failed to get pin 18 of the daisy!")
            .into_push_pull_output();

        let select2_pin = system
            .gpio
            .daisy19
            .take()
            .expect("Failed to get pin 19 of the daisy!")
            .into_push_pull_output();

        let muxed_parameters = dual_mux_4051::DualMux::new(
            system.adc1,
            mux1_pin,
            mux2_pin,
            select0_pin,
            select1_pin,
            select2_pin,
        );

        let mut adc2 = system.adc2.enable();
        adc2.set_resolution(adc::Resolution::SIXTEENBIT);
        let adc2_max_value = adc2.max_sample() as f32;

        let master_volume_pin = system
            .gpio
            .daisy21
            .take()
            .expect("Failed to get pin 13 of the daisy!")
            .into_analog();
        let master_volume = hid::AnalogControl::new(master_volume_pin, adc2_max_value);

        // ==============
        // CONFIG ENCODER
        // ==============

        let rotary_switch_pin = system
            .gpio
            .daisy28
            .take()
            .expect("Failed to get pin 28 of the daisy!")
            .into_floating_input();

        let rotary_clock_pin = system
            .gpio
            .daisy26
            .take()
            .expect("Failed to get pin 26 of the daisy!")
            .into_pull_up_input();

        let rotary_data_pin = system
            .gpio
            .daisy27
            .take()
            .expect("Failed to get pin 27 of the daisy!")
            .into_pull_up_input();

        let mut encoder =
            encoder::RotaryEncoder::new(rotary_switch_pin, rotary_clock_pin, rotary_data_pin);
        encoder.switch.set_held_thresh(Some(2));

        // ==================
        // CONFIG GATE INPUTS
        // ==================

        let gate1_pin = system
            .gpio
            .daisy24
            .take()
            .expect("Failed to get pin 24 of the daisy!")
            .into_floating_input();
        let gate1 = BinaryInput::new(gate1_pin, InputType::ActiveLow);

        let gate2_pin = system
            .gpio
            .daisy25
            .take()
            .expect("Failed to get pin 25 of the daisy!")
            .into_floating_input();
        let gate2 = BinaryInput::new(gate2_pin, InputType::ActiveLow);

        let gate3_pin = system
            .gpio
            .daisy22
            .take()
            .expect("Failed to get pin 22 of the daisy!")
            .into_floating_input();
        let gate3 = BinaryInput::new(gate3_pin, InputType::ActiveLow);

        let gate4_pin = system
            .gpio
            .daisy23
            .take()
            .expect("Failed to get pin 23 of the daisy!")
            .into_floating_input();

        let gate4 = BinaryInput::new(gate4_pin, InputType::ActiveLow);

        let kill_gate_pin = system
            .gpio
            .daisy20
            .take()
            .expect("Failed to get pin 20 of the daisy!")
            .into_floating_input();

        let kill_gate = BinaryInput::new(kill_gate_pin, InputType::ActiveLow);

        // ===========
        // CONFIG LEDs
        // ===========

        let mut led1 = system
            .gpio
            .daisy13
            .take()
            .expect("Failed to get pin 13 of the daisy!")
            .into_push_pull_output();
        led1.set_low().unwrap();

        let mut led2 = system
            .gpio
            .daisy14
            .take()
            .expect("Failed to get pin 14 of the daisy!")
            .into_push_pull_output();
        led2.set_low().unwrap();

        let mut led3 = system
            .gpio
            .daisy0
            .take()
            .expect("Failed to get pin 0 of the daisy!")
            .into_push_pull_output();
        led3.set_low().unwrap();

        // =============
        // CONFIG BUTTON
        // =============

        let button_pin = system
            .gpio
            .daisy9
            .take()
            .expect("Failed to get pin 9 of the daisy!")
            .into_pull_down_input();

        let button = BinaryInput::new(button_pin, InputType::ActiveHigh);

        // ===============
        // CONFIG FINISHED
        // ===============

        seed_led.set_low().unwrap();

        Self {
            audio_rate: AudioRate {
                audio: system.audio,
                buffer: [(0.0, 0.0); audio::BLOCK_SIZE_MAX],
            },
            control_rate: ControlRate {
                timer2: system.timer2,
                master_volume,
                muxed_parameters,
                gate1,
                gate2,
                gate3,
                gate4,
                kill_gate,
                led1,
                led2,
                led3,
                seed_led,
                button,
                encoder,
            },
            visual_rate: VisualRate { lcd, timer4 },
            sdram,
            // sd_card,
        }
    }
}
