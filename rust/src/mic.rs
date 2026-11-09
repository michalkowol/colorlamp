use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// Same window as the browser analyser (fftSize 2048).
pub const WINDOW: usize = 2048;

// The cpal stream is not Send on every platform, so it lives on its own thread until stopped.
pub struct MicCapture {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stop: Arc<AtomicBool>
}

impl MicCapture {
    pub fn start() -> Result<MicCapture> {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(WINDOW)));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let thread_buffer = buffer.clone();
        let thread_stop = stop.clone();
        std::thread::spawn(move || {
            let stream = match open_stream(thread_buffer) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            drop(stream);
        });
        ready_rx.recv().map_err(|_| anyhow!("microphone thread stopped"))??;
        Ok(MicCapture { buffer, stop })
    }

    pub fn samples(&self) -> Vec<f32> {
        self.buffer.lock().unwrap().iter().copied().collect()
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn open_stream(buffer: Arc<Mutex<VecDeque<f32>>>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| anyhow!("no input device"))?;
    let config = device.default_input_config()?;
    let channels = config.channels() as usize;
    let err_fn = |_err: cpal::Error| {};
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config.into(), channels, buffer, err_fn)?,
        SampleFormat::I16 => build::<i16>(&device, config.into(), channels, buffer, err_fn)?,
        SampleFormat::U16 => build::<u16>(&device, config.into(), channels, buffer, err_fn)?,
        SampleFormat::I32 => build::<i32>(&device, config.into(), channels, buffer, err_fn)?,
        SampleFormat::I8 => build::<i8>(&device, config.into(), channels, buffer, err_fn)?,
        other => return Err(anyhow!("unsupported sample format {other}"))
    };
    stream.play()?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    buffer: Arc<Mutex<VecDeque<f32>>>,
    err_fn: impl FnMut(cpal::Error) + Send + 'static
) -> Result<cpal::Stream>
where
    T: Sample + cpal::SizedSample + Send + 'static,
    f32: FromSample<T>
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut buf = buffer.lock().unwrap();
            // Down-mix to mono like the Web Audio analyser.
            for frame in data.chunks(channels.max(1)) {
                let sum: f32 = frame.iter().map(|s| f32::from_sample(*s)).sum();
                if buf.len() == WINDOW {
                    buf.pop_front();
                }
                buf.push_back(sum / frame.len() as f32);
            }
        },
        err_fn,
        None
    )?;
    Ok(stream)
}
