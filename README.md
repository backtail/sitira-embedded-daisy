# Sitira Embedded Daisy
*A granular synth developed for the eurorack environment based on the Electrosmith Daisy Seed platform*

---

### What is Sitira?
Sitira is a 34 HP eurorack module. It houses a granular synthesizer with a 2.2" screen to display the current waveform and the algorithm working through the audio source. Here is only the software implementation. The eurorack repo (with PCB design) can be found [here](https://github.com/backtail/sitira-eurorack)!

### What can I do with Sitira?
It has a few modes:
- Live input (record samples in real-time)
- Preloaded (stream wav files from a mirco SD card)
- Delay (real-time delay effect)

It comes with the following features/parameters:
- `Offset`
- `Grain Size`
- `Pitch`
- `Delay`
- `Velocity`
 
Those may be spreaded, which relatates to the idea of assigning random values to the parameters in a range.

For example, `Pitch` might be set to one specific value and every grain will be played back at the same speed. However, increase the `Pitch Spread` and the grains start to playback randomly at different speed (which corresponds to pitch). The higher `Pitch Spread` is increased, the higher and lower the pitches of individual grains become.

On top of that, depending on the `Grain Envelope` that has been chosen, a `Envelope Parameter` may be manipulated.

The best of it all, every parameter can be controlled by a knob, CV or both!
