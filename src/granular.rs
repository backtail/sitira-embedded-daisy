use libdaisy::AUDIO_SAMPLE_RATE;
use micromath::F32Ext;

const PI: f32 = 3.141592653589793;
const MAX_GRAINS: usize = 64;

#[derive(Clone, Copy)]
pub enum WindowFunction {
    Trapezodial,
    Gaussian,
    Sine,
    Hann,
    Hamming,
    Tukey,
}

#[derive(Clone, Copy)]
pub struct Grain {
    offset: u32,
    current_position: u32,
    gain: f32,
    size: f32,
    playhead: f32,
    sample_length: u32,
    pub done_with_window_funtion: bool,
    window: WindowFunction,
}

impl Grain {
    pub fn new(mut grain_length: f32, sample_length: u32, window_function: WindowFunction) -> Self {
        if grain_length < 1.0 {
            grain_length = 1.0;
        }
        Self {
            offset: 1_000,
            current_position: 0,
            gain: 0.7,
            size: (AUDIO_SAMPLE_RATE as f32 * grain_length) / 1000.0,
            playhead: 0.0,
            sample_length,
            done_with_window_funtion: true,
            window: window_function,
        }
    }

    pub fn update_sample_position(&mut self) -> usize {
        let position = self.offset + self.current_position;
        self.current_position += 5;
        if self.current_position > self.sample_length - self.size as u32 {
            self.current_position = 0;
            self.done_with_window_funtion = true;
        }
        position as usize
    }

    pub fn update_next_sample(&mut self, sample: f32) -> f32 {
        sample * self.update_window_function()
    }

    pub fn set_gain(&mut self, mut value: f32) {
        if value > 1.0 {
            value = 1.0;
        }
        if value < 0.0 {
            value = 0.0;
        }

        self.gain = value;
    }

    pub fn set_offset(&mut self, mut offset: u32) -> u32 {
        let max_offset = self.sample_length - self.size as u32;
        if offset > max_offset {
            offset = max_offset;
        }
        core::mem::replace(&mut self.offset, offset)
    }

    pub fn start_window_funtion(&mut self) {
        self.done_with_window_funtion = false;
        self.current_position = 0;
    }

    fn update_window_function(&mut self) -> f32 {
        let mut window_sample = 0.0_f32;

        match self.window {
            WindowFunction::Sine => window_sample = ((PI * self.playhead) / self.size).sin(),
            WindowFunction::Hann => {
                window_sample = 0.5 * (1.0 - ((2.0_f32 * PI * self.playhead) / self.size).cos())
            }
            WindowFunction::Hamming => {
                window_sample =
                    0.54 - 0.46 * ((2.0_f32 * PI * self.playhead) / self.size as f32).cos()
            }
            _ => (),
        }

        // update playhead if still inside the window funtion
        if !self.done_with_window_funtion {
            if self.playhead < self.size {
                self.playhead += 1.0;
            } else {
                self.playhead = 0.0;
                self.done_with_window_funtion = true;
            }
        }

        window_sample
    }
}

pub struct Grains {
    pub grains: [Grain; MAX_GRAINS],
    pub grain_size: f32,
    pub grain_size_spread: f32,
    pub active_grains: usize,
}

impl Grains {
    pub fn new(
        grain_amount: usize,
        grain_size_in_ms: f32,
        grain_size_spread_in_ms: f32,
        sample_length: u32,
        window_function: WindowFunction,
    ) -> Self {
        //  allocating all possible grains with dummy values
        let mut grains = [Grain::new(1.0, sample_length, window_function); MAX_GRAINS];

        // only updating first grains with values for actual use
        let mut active_grains = 0;
        if grain_amount > MAX_GRAINS {
            active_grains = MAX_GRAINS;
        }
        if grain_amount == 0 {
            active_grains = 1;
        }

        // calculating spread bewteen individual grains really poorly
        let grain_size_spread = grain_size_spread_in_ms / active_grains as f32;

        let grain_size = grain_size_in_ms;

        for instance in 0..active_grains {
            grains[instance] = Grain::new(
                instance as f32 * grain_size_spread + grain_size,
                sample_length,
                window_function,
            );
        }

        Self {
            grains,
            grain_size,
            grain_size_spread,
            active_grains,
        }
    }

    pub fn set_offset(&mut self, offset: u32) {
        for instance in 0..self.active_grains {
            self.grains[instance].set_offset(offset);
        }
    }

    pub fn start_window_funtion(&mut self) {
        for instance in 0..self.active_grains {
            self.grains[instance].start_window_funtion();
        }
    }
}
