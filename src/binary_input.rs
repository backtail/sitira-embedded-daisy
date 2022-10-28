use core::fmt::Debug;
use stm32h7xx_hal::hal::digital::v2::InputPin;

pub enum InputType {
    ActiveHigh,
    ActiveLow,
}

/// This is wrapper for `BinaryInput` devices.
/// Applies to buttons, switches, gate inputs, etc., which are being polled.
///
/// It takes away the need to remember how your input on a pin is configured
/// (`ActiveHigh` or `ActiveLow`). It also provides handy functions to perform
/// simple, but (sometimes really annoying) tasks, like giving information about
/// its switched state change.
pub struct BinaryInput<P> {
    pin: P,
    input_type: InputType,
    state: bool,
    transition: bool,
}

impl<P> BinaryInput<P>
where
    P: InputPin,
    <P as InputPin>::Error: Debug,
{
    /// Crerates a new `BinaryInput`. Can be configured as either `ActiveHigh` or `ActiveLow`.
    pub fn new(pin: P, input_type: InputType) -> Self {
        BinaryInput {
            pin,
            input_type,
            state: false,
            transition: false,
        }
    }

    /// Checks if the electrical input is high, depending on the `InputType`.
    /// - returns `true` if `ActiveHigh`
    /// - returns `false` if `ActiveLow`
    pub fn is_input_high(&self) -> bool {
        match self.input_type {
            InputType::ActiveHigh => self.pin.is_high().unwrap(),
            InputType::ActiveLow => self.pin.is_low().unwrap(),
        }
    }

    /// Checks if the electrical input is low, depending on the `InputType`.
    /// - returns `false` if `ActiveHigh`
    /// - returns `true` if `ActiveLow`
    pub fn is_input_low(&self) -> bool {
        !self.is_input_high()
    }

    /// Reads the electrical input, depending on the `InputType`.
    pub fn get_input_state(&self) -> bool {
        self.is_input_high()
    }

    /// Saves current state of the electrical input, depending on the `InputType`.
    ///
    /// Also performs a transition check.
    pub fn save_state(&mut self) {
        // checks if state has transition from low to high
        if self.get_input_state() != self.get_saved_state() && self.is_input_high() {
            self.transition = true;
        } else {
            self.transition = false;
        }
        self.state = self.is_input_high();
    }

    /// Returns the stored state.
    pub fn get_saved_state(&self) -> bool {
        self.state
    }

    /// Checks if the stored stated is high.
    pub fn is_saved_state_high(&self) -> bool {
        self.state
    }

    /// Checks if the stored stated is low.
    pub fn is_saved_state_low(&self) -> bool {
        !self.state
    }

    /// Returns `true` if the state changed from low to high (for one polling cycle).
    pub fn is_triggered(&self) -> bool {
        self.transition
    }

    // Returns `true` if the input is high, depending on the `InputType`.
    pub fn is_pressed(&self) -> bool {
        self.state
    }
}
