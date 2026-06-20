use anyhow::{bail, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample, Stream,
};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::SystemTime,
};

const OUTPUT_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug)]
pub struct Recording {
    pub started_at: SystemTime,
    pub stopped_at: SystemTime,
    pub wav: Vec<u8>,
}

#[derive(Default)]
pub struct Recorder {
    active: Option<ActiveRecording>,
}

struct ActiveRecording {
    started_at: SystemTime,
    sample_rate: u32,
    channels: u16,
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Stream,
}

impl Recorder {
    pub fn start(&mut self) -> Result<()> {
        if self.active.is_some() {
            bail!("recording is already active");
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device available")?;
        let config = device
            .default_input_config()
            .context("get default input config")?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let stream = build_stream(&device, &config, samples.clone())?;
        stream.play().context("start input stream")?;

        self.active = Some(ActiveRecording {
            started_at: SystemTime::now(),
            sample_rate,
            channels,
            samples,
            stream,
        });
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Recording> {
        let Some(active) = self.active.take() else {
            bail!("no active recording");
        };
        drop(active.stream);

        let stopped_at = SystemTime::now();
        let interleaved = {
            let samples = active
                .samples
                .lock()
                .map_err(|_| anyhow::anyhow!("recording sample buffer poisoned"))?;
            samples.clone()
        };
        let mono = mix_to_mono(&interleaved, active.channels);
        let resampled = resample_linear(&mono, active.sample_rate, OUTPUT_SAMPLE_RATE);
        let wav = wav_pcm16(&resampled, OUTPUT_SAMPLE_RATE)?;

        Ok(Recording {
            started_at: active.started_at,
            stopped_at,
            wav,
        })
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
) -> Result<Stream> {
    let err_fn = |err| tracing::warn!(error = ?err, "recording input stream error");
    match config.sample_format() {
        SampleFormat::I8 => input_stream::<i8>(device, config, samples, err_fn),
        SampleFormat::I16 => input_stream::<i16>(device, config, samples, err_fn),
        SampleFormat::I32 => input_stream::<i32>(device, config, samples, err_fn),
        SampleFormat::F32 => input_stream::<f32>(device, config, samples, err_fn),
        format => bail!("unsupported input sample format {format:?}"),
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream>
where
    T: Sample + SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            &config.clone().into(),
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if let Ok(mut buffer) = samples.lock() {
                    buffer.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                }
            },
            err_fn,
            None,
        )
        .context("build input stream")
}

fn mix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return interleaved.to_vec();
    }

    interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input_rate == output_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let output_len =
        (samples.len() as u64 * u64::from(output_rate) / u64::from(input_rate)) as usize;
    let mut out = Vec::with_capacity(output_len);
    let ratio = input_rate as f64 / output_rate as f64;
    for index in 0..output_len {
        let source = index as f64 * ratio;
        let left = source.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let frac = (source - left as f64) as f32;
        out.push(samples[left] * (1.0 - frac) + samples[right] * frac);
    }
    out
}

fn wav_pcm16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).context("create WAV writer")?;
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            writer.write_sample(value).context("write WAV sample")?;
        }
        writer.finalize().context("finalize WAV")?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_interleaved_stereo_to_mono() {
        assert_eq!(mix_to_mono(&[1.0, -1.0, 0.5, 0.25], 2), vec![0.0, 0.375]);
    }

    #[test]
    fn writes_16khz_mono_wav() {
        let wav = wav_pcm16(&[0.0, 0.5, -0.5], OUTPUT_SAMPLE_RATE).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            OUTPUT_SAMPLE_RATE
        );
    }
}
