//! Exercises the real default Windows endpoint with silent PCM.
use phoenix_audio::device::{PlaybackDevice, WaveOutput};
use std::{
    thread,
    time::{Duration, Instant},
};
fn main() -> std::io::Result<()> {
    let mut device = WaveOutput::open_default()?;
    device.pause(true)?;
    device.submit(&[0; 2048])?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(device.position()?, 0, "paused device advanced");
    device.pause(false)?;
    let start = Instant::now();
    while device.position()? < 2048 {
        if start.elapsed() > Duration::from_secs(5) {
            return Err(std::io::Error::other("device clock timeout"));
        }
        thread::sleep(Duration::from_millis(5));
    }
    device.submit(&[0; 2048])?;
    device.reset()?;
    assert_eq!(device.position()?, 0);
    println!("PASS default device: silent PCM presentation, pause/resume, synchronous reset");
    Ok(())
}
