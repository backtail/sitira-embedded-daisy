use core::fmt::Debug;
use nb::block;
use stm32h7xx_hal::adc::{Adc, AdcSampleTime, Disabled, Enabled, Resolution};
use stm32h7xx_hal::hal::adc::Channel;
use stm32h7xx_hal::hal::digital::v2::OutputPin;
use stm32h7xx_hal::stm32;

const MUX_INPUTS: usize = 8;

const ONE_BIT_MASK: u8 = 0b1;

pub struct DualMux<M1, M2, S0, S1, S2> {
    // HAL
    adc: Adc<stm32::ADC1, Enabled>,

    // PINS
    mux1_pin: M1,
    mux2_pin: M2,
    select0_pin: S0,
    select1_pin: S1,
    select2_pin: S2,

    // two 4051 Multiplexer
    value: [f32; MUX_INPUTS * 2],

    // helper
    conversion_value: f32,
}

impl<M1, M2, S0, S1, S2> DualMux<M1, M2, S0, S1, S2>
where
    M1: Channel<stm32::ADC1, ID = u8>,
    M2: Channel<stm32::ADC1, ID = u8>,
    S0: OutputPin,
    <S0 as OutputPin>::Error: Debug,
    S1: OutputPin,
    <S1 as OutputPin>::Error: Debug,
    S2: OutputPin,
    <S2 as OutputPin>::Error: Debug,
{
    pub fn new(
        adc: Adc<stm32::ADC1, Disabled>,
        mux1_pin: M1,
        mux2_pin: M2,
        select0_pin: S0,
        select1_pin: S1,
        select2_pin: S2,
    ) -> Self {
        // enable ADC
        let mut adc = adc.enable();
        adc.set_resolution(Resolution::SIXTEENBIT);
        adc.set_sample_time(AdcSampleTime::T_64);
        let conversion_value = 1.0 / adc.max_sample() as f32;

        DualMux {
            adc,

            mux1_pin,
            mux2_pin,
            select0_pin,
            select1_pin,
            select2_pin,

            value: [0.0; MUX_INPUTS * 2],

            conversion_value,
        }
    }

    fn set_select_pins(&mut self, input_number: usize) {
        let input_number = input_number.clamp(0, 15) as u8;
        let first_bit = input_number & ONE_BIT_MASK;
        let second_bit = (input_number >> 1) & ONE_BIT_MASK;
        let third_bit = (input_number >> 2) & ONE_BIT_MASK;

        match first_bit {
            0b0 => self.select0_pin.set_low().unwrap(),
            0b1 => self.select0_pin.set_high().unwrap(),
            _ => (),
        }

        match second_bit {
            0b0 => self.select1_pin.set_low().unwrap(),
            0b1 => self.select1_pin.set_high().unwrap(),
            _ => (),
        }

        match third_bit {
            0b0 => self.select2_pin.set_low().unwrap(),
            0b1 => self.select2_pin.set_high().unwrap(),
            _ => (),
        }
    }

    pub fn read_value(&mut self, input_number: usize) {
        match input_number {
            0..=8 => {
                self.set_select_pins(input_number);
                self.adc.start_conversion(&mut self.mux1_pin);
            }
            9..=16 => {
                self.set_select_pins(input_number);
                self.adc.start_conversion(&mut self.mux2_pin);
            }
            _ => (),
        }

        if let Ok(data) = block!(self.adc.read_sample()) {
            self.value[input_number] = data as f32 * self.conversion_value;
        }
    }

    pub fn get_value(&self, input_number: usize) -> f32 {
        self.value[input_number]
    }
}
