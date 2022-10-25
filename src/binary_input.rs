use core::fmt::Debug;
use stm32h7xx_hal::hal::digital::v2::InputPin;

pub enum InputType {
    ActiveHigh,
    ActiveLow,
}

pub struct BinaryInput<P> {
    pin: P,
    input_type: InputType,
}

impl<P> BinaryInput<P>
where
    P: InputPin,
    <P as InputPin>::Error: Debug,
{
    pub fn new(pin: P, input_type: InputType) -> Self {
        BinaryInput { pin, input_type }
    }

    pub fn is_low(&self) -> bool {
        match self.input_type {
            InputType::ActiveHigh => self.pin.is_low().unwrap(),
            InputType::ActiveLow => self.pin.is_high().unwrap(),
        }
    }

    pub fn is_high(&self) -> bool {
        match self.input_type {
            InputType::ActiveHigh => self.pin.is_high().unwrap(),
            InputType::ActiveLow => self.pin.is_low().unwrap(),
        }
    }
}
