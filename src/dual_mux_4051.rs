use core::fmt::Debug;
use stm32h7xx_hal::adc::{Adc, Disabled, Enabled};
use stm32h7xx_hal::hal::adc::Channel;
use stm32h7xx_hal::hal::digital::v2::OutputPin;
use stm32h7xx_hal::stm32;

const MUX_INPUTS: usize = 8;

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
        let adc = adc.enable();
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

    pub fn read_value(&mut self, input_number: usize) {
        match input_number {
            0..=8 => self.adc.start_conversion(&mut self.mux1_pin),
            9..=16 => self.adc.start_conversion(&mut self.mux2_pin),
            _ => (),
        }

        if let Ok(data) = self.adc.read_sample() {
            self.value[input_number] = data as f32 * self.conversion_value;
        }
    }

    pub fn get_value(&self, input_number: usize) -> f32 {
        self.value[input_number]
    }
}
