use libdaisy::AUDIO_SAMPLE_RATE;
use micromath::F32Ext;

// use micromath::{F32Ext, F32};
const PI: f32 = 3.141592653589793;

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
    gain: f32,
    relative_speed: f32,
    size: f32,
    playhead: f32,
    window: WindowFunction,
}

impl Grain {
    pub fn new(
        mut relative_speed: f32,
        mut grain_length: f32,
        window_function: WindowFunction,
    ) -> Self {
        if relative_speed > 8.0 {
            relative_speed = 8.0;
        }
        if relative_speed < -8.0 {
            relative_speed = -8.0;
        }
        if grain_length < 2.0 {
            grain_length = 2.0;
        }
        Self {
            gain: 1.0,
            relative_speed,
            size: (AUDIO_SAMPLE_RATE as f32 * grain_length) / 1000.0,
            playhead: 0.0,
            window: window_function,
        }
    }

    pub fn update_next(&mut self, sample: f32) -> f32 {
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

    fn update_window_function(&mut self) -> f32 {
        let mut window_sample = 0.0_f32;

        match self.window {
            WindowFunction::Sine => {
                window_sample = ((PI * self.playhead) / self.size).sin();
            }
            WindowFunction::Hann => {
                window_sample = 0.5 * (1.0 - ((2.0_f32 * PI * self.playhead) / self.size).cos());
            }
            WindowFunction::Hamming => {
                window_sample =
                    0.54 - 0.46 * ((2.0_f32 * PI * self.playhead) / self.size as f32).cos();
            }
            _ => (),
        }

        // update playhead
        if self.playhead < self.size {
            self.playhead += 1.0 * self.relative_speed;
        } else {
            self.playhead = 0.0;
        }

        window_sample
    }
}
