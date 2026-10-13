//! The signal processing around an MDX-Net vocal model.
//!
//! The model is only the learned core: it maps a spectrogram of the mix to
//! one of the vocals. The short-time Fourier transforms on either side are
//! ours and must match the ones it was trained with exactly: a periodic Hann
//! window, the signal reflected at both ends by half a window (PyTorch's
//! `center=True`), no normalisation, and the Nyquist bin left out.

use std::sync::Arc;

use anyhow::{Result, ensure};
use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use super::{FRAMES, HOP, Separator, WINDOW};

/// The learned core of an MDX-Net model.
pub trait Model: Send {
    /// Frequency bins the model takes, half the transform's length.
    fn bins(&self) -> usize;

    /// Maps the mix's spectrogram to the vocals'. Both are laid out as the
    /// model's `[1, 4, bins, FRAMES]` tensor: the left channel's real and
    /// imaginary parts, then the right's, each bin by frame.
    fn run(&mut self, mix: &[f32], vocals: &mut [f32]) -> Result<()>;
}

/// A separator built from a model and the transforms it expects.
pub struct Mdx<M> {
    model: M,
    bins: usize,
    size: usize,
    window: Vec<f32>,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    /// One channel, reflected at both ends.
    padded: Vec<f32>,
    frame: Vec<f32>,
    spectrum: Vec<Complex32>,
    scratch: Vec<Complex32>,
    mix: Vec<f32>,
    vocals: Vec<f32>,
    /// Overlap-added frames of one channel, and the window's own sum there.
    sum: Vec<f32>,
    weight: Vec<f32>,
}

impl<M: Model> Mdx<M> {
    pub fn new(model: M) -> Self {
        let bins = model.bins();
        let size = bins * 2;
        let mut planner = RealFftPlanner::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let scratch = forward
            .make_scratch_vec()
            .len()
            .max(inverse.make_scratch_vec().len());
        // Periodic, as `torch.hann_window` makes it.
        let window = (0..size)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / size as f32).cos())
            .collect::<Vec<_>>();
        let length = WINDOW + size;
        let mut weight = vec![0.0; length];
        for frame in 0..FRAMES {
            for (n, w) in window.iter().enumerate() {
                weight[frame * HOP + n] += w * w;
            }
        }
        Self {
            model,
            bins,
            size,
            window,
            spectrum: forward.make_output_vec(),
            forward,
            inverse,
            padded: vec![0.0; length],
            frame: vec![0.0; size],
            scratch: vec![Complex32::default(); scratch],
            mix: vec![0.0; 4 * bins * FRAMES],
            vocals: vec![0.0; 4 * bins * FRAMES],
            sum: vec![0.0; length],
            weight,
        }
    }

    /// Fills the model's input with one channel's spectrogram.
    fn analyse(&mut self, audio: &[f32], channel: usize) -> Result<()> {
        let pad = self.size / 2;
        for (i, slot) in self.padded.iter_mut().enumerate() {
            // Reflected without repeating the edge, as numpy's "reflect".
            let at = i as isize - pad as isize;
            let at = if at < 0 {
                -at
            } else if at >= WINDOW as isize {
                2 * (WINDOW as isize - 1) - at
            } else {
                at
            };
            *slot = audio[at as usize * 2 + channel];
        }
        let (re, im) = (2 * channel, 2 * channel + 1);
        for frame in 0..FRAMES {
            let start = frame * HOP;
            for ((slot, sample), w) in self
                .frame
                .iter_mut()
                .zip(&self.padded[start..start + self.size])
                .zip(&self.window)
            {
                *slot = sample * w;
            }
            self.forward
                .process_with_scratch(&mut self.frame, &mut self.spectrum, &mut self.scratch)
                .map_err(|error| anyhow::anyhow!("transform failed: {error}"))?;
            for (bin, value) in self.spectrum[..self.bins].iter().enumerate() {
                self.mix[(re * self.bins + bin) * FRAMES + frame] = value.re;
                self.mix[(im * self.bins + bin) * FRAMES + frame] = value.im;
            }
        }
        Ok(())
    }

    /// Writes one channel of the model's output back as sound.
    fn synthesise(&mut self, out: &mut [f32], channel: usize) -> Result<()> {
        let (re, im) = (2 * channel, 2 * channel + 1);
        self.sum.fill(0.0);
        let scale = 1.0 / self.size as f32;
        for frame in 0..FRAMES {
            for (bin, value) in self.spectrum.iter_mut().enumerate() {
                *value = if bin < self.bins {
                    Complex32::new(
                        self.vocals[(re * self.bins + bin) * FRAMES + frame],
                        self.vocals[(im * self.bins + bin) * FRAMES + frame],
                    )
                } else {
                    Complex32::default()
                };
            }
            // The inverse transform wants a real DC bin.
            self.spectrum[0].im = 0.0;
            self.inverse
                .process_with_scratch(&mut self.spectrum, &mut self.frame, &mut self.scratch)
                .map_err(|error| anyhow::anyhow!("transform failed: {error}"))?;
            let start = frame * HOP;
            for ((sum, sample), w) in self.sum[start..start + self.size]
                .iter_mut()
                .zip(&self.frame)
                .zip(&self.window)
            {
                *sum += sample * scale * w;
            }
        }
        let pad = self.size / 2;
        for (i, (sum, weight)) in self.sum[pad..pad + WINDOW]
            .iter()
            .zip(&self.weight[pad..pad + WINDOW])
            .enumerate()
        {
            out[i * 2 + channel] = sum / weight.max(1e-8);
        }
        Ok(())
    }
}

impl<M: Model> Separator for Mdx<M> {
    fn vocals(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
        ensure!(audio.len() == WINDOW * 2, "expected {WINDOW} stereo frames");
        self.analyse(audio, 0)?;
        self.analyse(audio, 1)?;
        self.model.run(&self.mix, &mut self.vocals)?;
        let mut out = vec![0.0; WINDOW * 2];
        self.synthesise(&mut out, 0)?;
        self.synthesise(&mut out, 1)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hands the mix straight back, so the transforms alone are measured.
    struct Identity(usize);

    impl Model for Identity {
        fn bins(&self) -> usize {
            self.0
        }

        fn run(&mut self, mix: &[f32], vocals: &mut [f32]) -> Result<()> {
            vocals.copy_from_slice(mix);
            Ok(())
        }
    }

    fn tone(hz: [f32; 2]) -> Vec<f32> {
        (0..WINDOW)
            .flat_map(|n| {
                let t = n as f32 / 44_100.0;
                hz.map(|hz| 0.5 * (std::f32::consts::TAU * hz * t).sin())
            })
            .collect()
    }

    #[test]
    fn the_transforms_give_back_what_they_took() {
        for bins in [2048, 3072] {
            let audio = tone([440.0, 1_250.0]);
            let out = Mdx::new(Identity(bins)).vocals(&audio).unwrap();
            let worst = audio
                .iter()
                .zip(&out)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(worst < 1e-3, "{bins} bins: off by {worst}");
        }
    }

    #[test]
    fn the_model_sees_each_channel_in_its_own_planes() {
        struct Check;
        impl Model for Check {
            fn bins(&self) -> usize {
                2048
            }
            fn run(&mut self, mix: &[f32], vocals: &mut [f32]) -> Result<()> {
                let plane = 2048 * FRAMES;
                let energy = |p: usize| {
                    mix[p * plane..(p + 1) * plane]
                        .iter()
                        .map(|v| v * v)
                        .sum::<f32>()
                };
                // Silence on the left, a tone on the right.
                assert_eq!(energy(0) + energy(1), 0.0);
                assert!(energy(2) + energy(3) > 1.0);
                vocals.fill(0.0);
                Ok(())
            }
        }
        let audio = tone([0.0, 440.0]);
        let out = Mdx::new(Check).vocals(&audio).unwrap();
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
