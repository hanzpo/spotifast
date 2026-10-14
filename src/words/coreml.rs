//! The letter model through Core ML: wav2vec2-base-960h, which hears
//! speech (and, well enough, singing) as letters, fifty times a second.
//!
//! It is FinDIT Studio's conversion
//! (<https://huggingface.co/FinDIT-Studio/alignkit-coreml>, Apache-2.0) of
//! Facebook's model, made for forced alignment: its log-probabilities are
//! exact on the CPU, where it is also fastest, about half a second a minute.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

use anyhow::{Result, anyhow, bail, ensure};
use half::f16;
use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLMultiArray, MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use super::align::{Emissions, LABELS};
use crate::karaoke::coreml::{error, model, strides_of};
use crate::karaoke::model::Package;

/// The model's package, pinned with its digests.
pub const ALIGNER: Package = Package {
    repository: "https://huggingface.co/FinDIT-Studio/alignkit-coreml/resolve",
    revision: "a144c29f9a8999fe3594e682b6298f4ce14fe154",
    name: "base960h_aligner.mlpackage",
    files: &[
        (
            "Manifest.json",
            "bae000742a392d10914bce5d02bcb86ba4bbda83de8c7a141d27c79035fab0d3",
        ),
        (
            "Data/com.apple.CoreML/model.mlmodel",
            "ff4b7c3781d2a07698cb8883050697a45abb9e21325da3d9254df35139553c9c",
        ),
        (
            "Data/com.apple.CoreML/weights/weight.bin",
            "de51193fe73fb3aad085f9c794f08bfde1b939fc12f92e0834edcd4cb712e642",
        ),
    ],
};

/// The rate the model hears at.
pub const SAMPLE_RATE: u32 = 16_000;
/// Samples the model takes at once: a minute.
const WINDOW: usize = 960_000;
/// Samples each frame of emissions covers.
const HOP: usize = 320;
/// Frames a full window gives.
const FRAMES: usize = 2999;
/// Seconds kept at each side of a window's seam, where the model hears
/// less of what surrounds a sound.
const OVERLAP: usize = 2 * SAMPLE_RATE as usize;

pub struct Aligner {
    model: Retained<MLModel>,
    input: Retained<MLMultiArray>,
    name: Retained<NSString>,
    output: Retained<NSString>,
}

impl Aligner {
    pub fn load(package: &Path) -> Result<Self> {
        autoreleasepool(|_| {
            let model = model(package, MLComputeUnits::CPUOnly)?;
            let shape = NSArray::from_retained_slice(&[
                NSNumber::new_usize(1),
                NSNumber::new_usize(WINDOW),
            ]);
            let input = unsafe {
                MLMultiArray::initWithShape_dataType_error(
                    MLMultiArray::alloc(),
                    &shape,
                    MLMultiArrayDataType::Float32,
                )
            }
            .map_err(|failure| error(&failure))?;
            Ok(Self {
                model,
                input,
                name: NSString::from_str("waveform"),
                output: NSString::from_str("emissions"),
            })
        })
    }

    /// The emissions of a whole song, mono at [`SAMPLE_RATE`]. It is heard
    /// a minute at a time; windows overlap, and each frame is taken from
    /// the window that hears most around it.
    pub fn emissions(&mut self, audio: &[f32]) -> Result<Emissions> {
        let frames = audio.len().div_ceil(HOP);
        let mut data = Vec::with_capacity(frames * LABELS.len());
        let mut start = 0;
        loop {
            let window = &audio[start..audio.len().min(start + WINDOW)];
            let heard = autoreleasepool(|_| self.predict(window))?;
            let last = start + WINDOW >= audio.len();
            // Frames up to the seam, which sits an overlap before the end.
            let from = data.len() / LABELS.len() - start / HOP;
            let to = if last {
                window.len().div_ceil(HOP).min(FRAMES)
            } else {
                (WINDOW - OVERLAP) / HOP
            };
            data.extend_from_slice(&heard[from * LABELS.len()..to * LABELS.len()]);
            if last {
                break;
            }
            start += WINDOW - 2 * OVERLAP;
        }
        Ok(Emissions { data })
    }

    fn predict(&mut self, window: &[f32]) -> Result<Vec<f32>> {
        let write = block2::StackBlock::new(
            |bytes: NonNull<c_void>, _: isize, _: NonNull<NSArray<NSNumber>>| {
                let data = bytes.cast::<f32>().as_ptr();
                for at in 0..WINDOW {
                    let sample = window.get(at).copied().unwrap_or_default();
                    unsafe { data.add(at).write(sample) };
                }
            },
        );
        unsafe { self.input.getMutableBytesWithHandler(&write) };
        let value = unsafe { MLFeatureValue::featureValueWithMultiArray(&self.input) };
        let features = NSDictionary::from_retained_objects(
            &[&*self.name],
            &[Retained::into_super(Retained::into_super(value))],
        );
        let provider = unsafe {
            MLDictionaryFeatureProvider::initWithDictionary_error(
                MLDictionaryFeatureProvider::alloc(),
                &features,
            )
        }
        .map_err(|failure| error(&failure))?;
        let provider = ProtocolObject::from_ref(&*provider);
        let result = unsafe { self.model.predictionFromFeatures_error(provider) }
            .map_err(|failure| error(&failure))?;
        let output = unsafe { result.featureValueForName(&self.output) }
            .and_then(|value| unsafe { value.multiArrayValue() })
            .ok_or_else(|| anyhow!("the letter model gave no output"))?;
        read(&output)
    }
}

/// Copies the `[1, FRAMES, LABELS]` output out, whatever its strides.
fn read(array: &MLMultiArray) -> Result<Vec<f32>> {
    let shape = strides_of(&*unsafe { array.shape() });
    ensure!(
        shape == [1, FRAMES, LABELS.len()],
        "the letter model gave {shape:?}"
    );
    let strides = strides_of(&*unsafe { array.strides() });
    let kind = unsafe { array.dataType() };
    let half = kind == MLMultiArrayDataType::Float16;
    if !half && kind != MLMultiArrayDataType::Float32 {
        bail!("the letter model gave numbers of type {}", kind.0);
    }
    let out = std::cell::RefCell::new(vec![0.0; FRAMES * LABELS.len()]);
    let copy = block2::StackBlock::new(|bytes: NonNull<c_void>, _: isize| {
        for (i, slot) in out.borrow_mut().iter_mut().enumerate() {
            let at = i / LABELS.len() * strides[1] + i % LABELS.len() * strides[2];
            *slot = unsafe {
                if half {
                    bytes.cast::<f16>().add(at).read().to_f32()
                } else {
                    bytes.cast::<f32>().add(at).read()
                }
            };
        }
    });
    unsafe { array.getBytesWithHandler(&copy) };
    Ok(out.into_inner())
}
