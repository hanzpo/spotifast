//! The vocal model through Core ML, on the Neural Engine.
//!
//! The model is UVR's MDX-Net "Voc FT", converted to an fp16 Core ML
//! program. Core ML compiles it for this Mac the first time; the compiled
//! copy is kept next to it so later loads take a moment.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use anyhow::{Context, Result, anyhow, bail, ensure};
use half::f16;
use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLModelConfiguration, MLMultiArray, MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

use super::{FRAMES, Mdx, Model, Separator};

/// Frequency bins of the Voc FT model, half its 6144-point transform.
const BINS: usize = 3072;

/// The model's package, and where its compiled form is kept.
pub(crate) fn compiled_path(package: &Path) -> PathBuf {
    package.with_extension("mlmodelc")
}

/// Loads the model in `package` as a separator.
pub fn load(package: &Path) -> Result<Box<dyn Separator>> {
    let model = autoreleasepool(|_| CoreMl::load(package))?;
    Ok(Box::new(Mdx::new(model)))
}

struct CoreMl {
    model: Retained<MLModel>,
    input: Retained<MLMultiArray>,
    name: Retained<NSString>,
    output: Retained<NSString>,
}

// SAFETY: Core ML's models and arrays may be used from any thread, one at
// a time; this one stays on the karaoke thread that made it.
unsafe impl Send for CoreMl {}

pub(crate) fn url(path: &Path) -> Result<Retained<NSURL>> {
    let text = path
        .to_str()
        .ok_or_else(|| anyhow!("{} is not UTF-8", path.display()))?;
    Ok(NSURL::fileURLWithPath(&NSString::from_str(text)))
}

pub(crate) fn error(error: &objc2_foundation::NSError) -> anyhow::Error {
    anyhow!("{}", error.localizedDescription())
}

/// Loads the Core ML package at `package` to run on `units`, compiling it
/// for this Mac the first time and keeping the compiled copy beside it.
pub(crate) fn model(package: &Path, units: MLComputeUnits) -> Result<Retained<MLModel>> {
    let compiled = compiled_path(package);
    if !compiled.is_dir() {
        compile(package, &compiled)?;
    }
    let configuration = unsafe { MLModelConfiguration::new() };
    unsafe { configuration.setComputeUnits(units) };
    let model = unsafe {
        MLModel::modelWithContentsOfURL_configuration_error(&*url(&compiled)?, &configuration)
    };
    Ok(match model {
        Ok(model) => model,
        Err(failure) => {
            // A compiled copy from another system version can refuse to
            // load; compiling again fixes that.
            log::warn!("compiling {} again: {}", package.display(), error(&failure));
            std::fs::remove_dir_all(&compiled).ok();
            compile(package, &compiled)?;
            unsafe {
                MLModel::modelWithContentsOfURL_configuration_error(
                    &*url(&compiled)?,
                    &configuration,
                )
            }
            .map_err(|failure| error(&failure))
            .with_context(|| format!("loading {}", package.display()))?
        }
    })
}

impl CoreMl {
    fn load(package: &Path) -> Result<Self> {
        let model = model(package, MLComputeUnits::CPUAndNeuralEngine)?;
        let shape = NSArray::from_retained_slice(&[
            NSNumber::new_usize(1),
            NSNumber::new_usize(4),
            NSNumber::new_usize(BINS),
            NSNumber::new_usize(FRAMES),
        ]);
        let input = unsafe {
            MLMultiArray::initWithShape_dataType_error(
                MLMultiArray::alloc(),
                &shape,
                MLMultiArrayDataType::Float16,
            )
        }
        .map_err(|failure| error(&failure))?;
        Ok(Self {
            model,
            input,
            name: NSString::from_str("input"),
            output: NSString::from_str("output"),
        })
    }
}

pub(crate) fn compile(package: &Path, compiled: &Path) -> Result<()> {
    #[allow(deprecated)] // The asynchronous form only adds a block to wait on.
    let made = unsafe { MLModel::compileModelAtURL_error(&*url(package)?) }
        .map_err(|failure| error(&failure))
        .context("compiling the vocal model")?;
    let made = made
        .path()
        .map(|path| PathBuf::from(path.to_string()))
        .ok_or_else(|| anyhow!("Core ML compiled the model to nowhere"))?;
    // Core ML leaves it in a temporary folder, perhaps on another volume.
    if std::fs::rename(&made, compiled).is_err() {
        copy_dir(&made, compiled)?;
        std::fs::remove_dir_all(&made).ok();
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

impl Model for CoreMl {
    fn bins(&self) -> usize {
        BINS
    }

    fn run(&mut self, mix: &[f32], vocals: &mut [f32]) -> Result<()> {
        autoreleasepool(|_| self.predict(mix, vocals))
    }
}

impl CoreMl {
    fn predict(&mut self, mix: &[f32], vocals: &mut [f32]) -> Result<()> {
        let count = 4 * BINS * FRAMES;
        ensure!(mix.len() == count && vocals.len() == count);
        let write = block2::StackBlock::new(
            |bytes: NonNull<c_void>, _: isize, strides: NonNull<NSArray<NSNumber>>| {
                let strides = strides_of(unsafe { strides.as_ref() });
                let data = bytes.cast::<f16>().as_ptr();
                for (i, value) in mix.iter().enumerate() {
                    let (plane, bin, frame) = (i / (BINS * FRAMES), i / FRAMES % BINS, i % FRAMES);
                    let at = plane * strides[1] + bin * strides[2] + frame * strides[3];
                    unsafe { data.add(at).write(f16::from_f32(*value)) };
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
            .map_err(|failure| error(&failure))
            .context("running the vocal model")?;
        let output = unsafe { result.featureValueForName(&self.output) }
            .and_then(|value| unsafe { value.multiArrayValue() })
            .ok_or_else(|| anyhow!("the vocal model gave no output"))?;
        read(&output, vocals)
    }
}

pub(crate) fn strides_of(strides: &NSArray<NSNumber>) -> Vec<usize> {
    strides.iter().map(|n| n.as_usize()).collect()
}

/// Copies a `[1, 4, BINS, FRAMES]` array out, whatever its strides.
fn read(array: &MLMultiArray, out: &mut [f32]) -> Result<()> {
    let shape = strides_of(&*unsafe { array.shape() });
    ensure!(
        shape == [1, 4, BINS, FRAMES],
        "the vocal model gave {shape:?}"
    );
    let strides = strides_of(&*unsafe { array.strides() });
    let kind = unsafe { array.dataType() };
    let half = kind == MLMultiArrayDataType::Float16;
    if !half && kind != MLMultiArrayDataType::Float32 {
        bail!("the vocal model gave numbers of type {}", kind.0);
    }
    let out = std::cell::RefCell::new(out);
    let copy = block2::StackBlock::new(|bytes: NonNull<c_void>, _: isize| {
        let mut out = out.borrow_mut();
        for (i, slot) in out.iter_mut().enumerate() {
            let (plane, bin, frame) = (i / (BINS * FRAMES), i / FRAMES % BINS, i % FRAMES);
            let at = plane * strides[1] + bin * strides[2] + frame * strides[3];
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::karaoke::{Karaoke, Stage};

    fn raw(path: &str) -> Vec<f32> {
        std::fs::read(path)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    /// Runs the real model over a song through the stage and compares the
    /// voice it takes out with an offline separation of the same song.
    /// `KARAOKE_MODEL` is the `.mlpackage`, `KARAOKE_SONG` and
    /// `KARAOKE_VOCALS` raw little-endian stereo `f32`.
    #[test]
    #[ignore = "needs the model and a song"]
    fn the_model_takes_out_the_voice_an_offline_run_finds() {
        let model = PathBuf::from(std::env::var("KARAOKE_MODEL").unwrap());
        let song = raw(&std::env::var("KARAOKE_SONG").unwrap());
        let reference = raw(&std::env::var("KARAOKE_VOCALS").unwrap());
        let control = Karaoke::shared();
        control.set_loader(Some(Arc::new(move || load(&model))));
        control.set_enabled(true);
        let mut stage = Stage::new(Arc::clone(&control));
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        for packet in song.chunks(2048) {
            out.extend(stage.process(packet.iter().map(|&s| f64::from(s)).collect()));
            // As an output would take it, but ten times as fast.
            std::thread::sleep(std::time::Duration::from_secs_f64(
                packet.len() as f64 / 2.0 / 44_100.0 / 10.0,
            ));
        }
        control.track_ended();
        out.extend(stage.finish());
        let took = started.elapsed();
        assert_eq!(control.status(), crate::karaoke::Status::Running);
        assert_eq!(out.len(), song.len());
        // The voice the stage took out, once the model has loaded: at ten
        // times the pace, its second of loading lets ten of the song by.
        let skip = 44_100 * 2 * 20;
        let (mut signal, mut noise) = (0.0f64, 0.0f64);
        for i in skip..song.len() {
            let removed = f64::from(song[i]) - out[i];
            signal += f64::from(reference[i]).powi(2);
            noise += (f64::from(reference[i]) - removed).powi(2);
        }
        let agreement = 10.0 * (signal / noise).log10();
        eprintln!("{agreement:.1} dB from the offline voice; {took:?} for the song");
        assert!(agreement > 15.0, "{agreement:.1} dB");
    }
}
