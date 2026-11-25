//! Lightweight UI / new-mail tones via rodio (background thread).

use std::sync::mpsc::{self, SyncSender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use mailbox_core::SoundTheme;
use rodio::source::{SineWave, Source};
use rodio::{OutputStream, Sink};

#[derive(Clone, Copy, Debug)]
enum SoundKind {
    NewMail,
    Click,
}

#[derive(Clone, Copy, Debug)]
struct SoundEvent {
    kind: SoundKind,
    theme: SoundTheme,
}

static TX: OnceLock<SyncSender<SoundEvent>> = OnceLock::new();

fn sender() -> Option<&'static SyncSender<SoundEvent>> {
    Some(TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<SoundEvent>(8);
        thread::Builder::new()
            .name("ailiak-sounds".into())
            .spawn(move || {
                let Ok((_stream, handle)) = OutputStream::try_default() else {
                    tracing::debug!("no audio output — sounds disabled");
                    while rx.recv().is_ok() {}
                    return;
                };
                while let Ok(ev) = rx.recv() {
                    if let Err(e) = play_on(&handle, ev) {
                        tracing::debug!(error = %e, ?ev, "sound play failed");
                    }
                }
            })
            .ok();
        tx
    }))
}

fn play_on(handle: &rodio::OutputStreamHandle, ev: SoundEvent) -> Result<(), String> {
    let sink = Sink::try_new(handle).map_err(|e| e.to_string())?;
    match (ev.theme, ev.kind) {
        // Calm — original soft mid two-note / short tick
        (SoundTheme::Calm, SoundKind::Click) => {
            append_tone(&sink, 920.0, 38, 0.12, 4, 0);
        }
        (SoundTheme::Calm, SoundKind::NewMail) => {
            append_tone(&sink, 660.0, 110, 0.18, 8, 0);
            append_tone(&sink, 880.0, 160, 0.16, 8, 90);
        }
        // Bright — higher sparkle
        (SoundTheme::Bright, SoundKind::Click) => {
            append_tone(&sink, 1400.0, 28, 0.10, 2, 0);
            append_tone(&sink, 1760.0, 36, 0.08, 2, 22);
        }
        (SoundTheme::Bright, SoundKind::NewMail) => {
            append_tone(&sink, 880.0, 80, 0.14, 6, 0);
            append_tone(&sink, 1174.0, 90, 0.14, 6, 70);
            append_tone(&sink, 1568.0, 140, 0.12, 6, 150);
        }
        // Soft — warmer / lower
        (SoundTheme::Soft, SoundKind::Click) => {
            append_tone(&sink, 480.0, 45, 0.14, 6, 0);
        }
        (SoundTheme::Soft, SoundKind::NewMail) => {
            append_tone(&sink, 392.0, 140, 0.16, 12, 0);
            append_tone(&sink, 523.0, 180, 0.14, 12, 110);
        }
    }
    sink.sleep_until_end();
    Ok(())
}

fn append_tone(
    sink: &Sink,
    freq: f32,
    ms: u64,
    amp: f32,
    fade_ms: u64,
    delay_ms: u64,
) {
    let tone = SineWave::new(freq)
        .take_duration(Duration::from_millis(ms))
        .amplify(amp)
        .fade_in(Duration::from_millis(fade_ms));
    if delay_ms > 0 {
        sink.append(tone.delay(Duration::from_millis(delay_ms)));
    } else {
        sink.append(tone);
    }
}

fn enqueue(kind: SoundKind, theme: SoundTheme) {
    let Some(tx) = sender() else {
        return;
    };
    let _ = tx.try_send(SoundEvent { kind, theme });
}

pub fn play_new_mail(theme: SoundTheme) {
    enqueue(SoundKind::NewMail, theme);
}

pub fn play_click(theme: SoundTheme) {
    enqueue(SoundKind::Click, theme);
}
