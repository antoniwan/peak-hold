//! Reads the default microphone through the OS audio system (WASAPI on Windows,
//! Core Audio on macOS) and turns it into one level reading every ~33 ms.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use tauri::AppHandle;

/// Length of one level reading. Short enough to catch a clap.
const BLOCK_SECS: f32 = 0.033;
/// How often the tray tooltip may change.
const TRAY_EVERY: Duration = Duration::from_millis(250);

pub fn spawn(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<f32>();

    // cpal streams can't move between threads on every platform, so one thread
    // opens the stream and keeps it alive for the life of the app.
    let audio_app = app.clone();
    thread::spawn(move || match open_stream(tx) {
        Ok((stream, device)) => {
            let _keep_alive = stream;
            crate::on_started(&audio_app, device);
            loop {
                thread::park();
            }
        }
        Err(message) => crate::on_error(&audio_app, message),
    });

    // Levels leave the audio callback through a channel so the callback never
    // waits on the UI.
    thread::spawn(move || {
        let mut last_tray = Instant::now();
        loop {
            match rx.recv_timeout(TRAY_EVERY) {
                Ok(level_fs) => crate::on_level(&app, level_fs),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if last_tray.elapsed() >= TRAY_EVERY {
                crate::flush_tray(&app);
                last_tray = Instant::now();
            }
        }
    });
}

fn open_stream(tx: Sender<f32>) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No microphone was found. Plug one in, then restart Peak Hold.")?;
    let name = device.name().unwrap_or_else(|_| "Microphone".into());
    let supported = device
        .default_input_config()
        .map_err(|e| format!("The microphone couldn't be opened ({e})."))?;

    let config: cpal::StreamConfig = supported.config();
    let block = ((config.sample_rate.0 as f32 * BLOCK_SECS) as usize).max(1) * config.channels as usize;

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, tx, block),
        SampleFormat::I16 => build::<i16>(&device, &config, tx, block),
        SampleFormat::U16 => build::<u16>(&device, &config, tx, block),
        SampleFormat::I32 => build::<i32>(&device, &config, tx, block),
        other => return Err(format!("This microphone uses a sample format Peak Hold can't read ({other:?}).")),
    }
    .map_err(|e| format!("The microphone couldn't be opened ({e})."))?;

    stream
        .play()
        .map_err(|e| format!("The microphone couldn't start ({e}). On Windows, check Settings > Privacy & security > Microphone."))?;
    Ok((stream, name))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender<f32>,
    block: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut sum = 0f64;
    let mut count = 0usize;
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for &sample in data {
                let v = sample.to_sample::<f32>() as f64;
                sum += v * v;
                count += 1;
                if count >= block {
                    let rms = (sum / count as f64).sqrt();
                    let _ = tx.send((20.0 * rms.max(1e-10).log10()) as f32);
                    sum = 0.0;
                    count = 0;
                }
            }
        },
        |err| eprintln!("audio stream error: {err}"),
        None,
    )
}
