//! Synthesises the bundled sound effects into `assets/sounds/` (original,
//! covered by the app licence). Deterministic: rerunning gives the same files.
//! `cargo run -p rw-assets --example make_sounds`

use std::f32::consts::TAU;
use std::path::Path;

const RATE: u32 = 48_000;

/// Repeatable noise (xorshift).
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

fn secs(n: usize) -> f32 {
    n as f32 / RATE as f32
}

fn len(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// Fast attack, exponential decay.
fn env(t: f32, attack: f32, decay: f32) -> f32 {
    if t < attack {
        t / attack
    } else {
        (-(t - attack) / decay).exp()
    }
}

fn pop() -> Vec<f32> {
    (0..len(0.18))
        .map(|n| {
            let t = secs(n);
            let f = 900.0 * (-t * 25.0).exp() + 180.0;
            (TAU * f * t).sin() * env(t, 0.002, 0.04) * 0.8
        })
        .collect()
}

fn click() -> Vec<f32> {
    let mut noise = Noise(7);
    (0..len(0.05))
        .map(|n| {
            let t = secs(n);
            (noise.next() * 0.5 + (TAU * 2400.0 * t).sin() * 0.5) * env(t, 0.0005, 0.006) * 0.7
        })
        .collect()
}

/// Band-passed noise sweeping in frequency, louder in the middle.
fn sweep(seconds: f32, f0: f32, f1: f32, seed: u32) -> Vec<f32> {
    let mut noise = Noise(seed);
    let (mut lp, mut bp) = (0.0f32, 0.0f32);
    let n = len(seconds);
    (0..n)
        .map(|i| {
            let x = i as f32 / n as f32;
            let f = f0 * (f1 / f0).powf(x);
            // State-variable filter.
            let g = (std::f32::consts::PI * f / RATE as f32).tan();
            let hp = noise.next() - lp - 1.4 * bp;
            bp += g * hp;
            lp += g * bp;
            let shape = (std::f32::consts::PI * x).sin().powf(1.5);
            bp * shape * 0.9
        })
        .collect()
}

/// A bell: inharmonic partials with their own decays.
fn bell(f: f32, seconds: f32) -> Vec<f32> {
    let partials = [
        (1.0, 1.0, 1.2),
        (2.76, 0.5, 0.6),
        (5.4, 0.25, 0.3),
        (8.9, 0.12, 0.15),
    ];
    (0..len(seconds))
        .map(|n| {
            let t = secs(n);
            partials
                .iter()
                .map(|(m, a, d)| (TAU * f * m * t).sin() * a * env(t, 0.003, *d))
                .sum::<f32>()
                * 0.35
        })
        .collect()
}

fn mix_at(out: &mut Vec<f32>, sound: &[f32], at: f32) {
    let start = len(at);
    if out.len() < start + sound.len() {
        out.resize(start + sound.len(), 0.0);
    }
    for (i, s) in sound.iter().enumerate() {
        out[start + i] += s;
    }
}

fn chime() -> Vec<f32> {
    let mut out = Vec::new();
    for (i, f) in [523.25, 659.25, 783.99, 1046.5].iter().enumerate() {
        mix_at(&mut out, &bell(*f, 1.2), i as f32 * 0.09);
    }
    out
}

fn notify() -> Vec<f32> {
    let mut out = Vec::new();
    mix_at(&mut out, &bell(880.0, 0.7), 0.0);
    mix_at(&mut out, &bell(1318.5, 0.9), 0.14);
    out
}

fn kick() -> Vec<f32> {
    (0..len(0.45))
        .map(|n| {
            let t = secs(n);
            // Pitch drops fast; phase accumulates the falling frequency.
            let phase = TAU * (50.0 * t + 110.0 / 30.0 * (1.0 - (-t * 30.0).exp()));
            phase.sin() * env(t, 0.001, 0.15)
        })
        .collect()
}

fn write_wav(path: &Path, mono: &[f32]) {
    // Normalise to −1 dBFS, stereo 16-bit.
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    let gain = 0.89 / peak;
    let data: Vec<u8> = mono
        .iter()
        .flat_map(|v| {
            let s = ((v * gain).clamp(-1.0, 1.0) * 32767.0).round() as i16;
            let b = s.to_le_bytes();
            [b[0], b[1], b[0], b[1]]
        })
        .collect();
    let mut w = Vec::new();
    w.extend(b"RIFF");
    w.extend(&(36 + data.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(&16u32.to_le_bytes());
    w.extend(&1u16.to_le_bytes()); // PCM
    w.extend(&2u16.to_le_bytes()); // channels
    w.extend(&RATE.to_le_bytes());
    w.extend(&(RATE * 4).to_le_bytes());
    w.extend(&4u16.to_le_bytes());
    w.extend(&16u16.to_le_bytes());
    w.extend(b"data");
    w.extend(&(data.len() as u32).to_le_bytes());
    w.extend(&data);
    std::fs::write(path, w).unwrap();
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
    std::fs::create_dir_all(&dir).unwrap();
    let sounds: [(&str, Vec<f32>); 9] = [
        ("pop", pop()),
        ("click", click()),
        ("whoosh", sweep(0.7, 300.0, 4000.0, 11)),
        ("swoosh-down", sweep(0.6, 5000.0, 400.0, 23)),
        ("riser", sweep(2.5, 150.0, 8000.0, 31)),
        ("ding", bell(1046.5, 1.4)),
        ("chime", chime()),
        ("notify", notify()),
        ("thump", kick()),
    ];
    for (name, s) in sounds {
        write_wav(&dir.join(format!("{name}.wav")), &s);
        println!("{name}: {:.2} s", s.len() as f32 / RATE as f32);
    }
}
