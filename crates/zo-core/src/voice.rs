//! Voice to text with the engine Zohara OS already ships for voice typing (whisper.cpp and its model, from the packages
//! `zohara-voice` and `zohara-voice-model`). Recording goes through `parecord`; nothing leaves the computer.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub const MODEL: &str = "/usr/share/zohara/dictation/ggml-base-q8_0.bin";
const WHISPER_DIR: &str = "/usr/lib/zohara/whisper";
const RATE: usize = 16_000;
/// A safety stop so a forgotten microphone does not record for ever.
const MAX_SECONDS: usize = 300;

fn engine() -> Option<PathBuf> {
    let flags = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| c.lines().find(|l| l.starts_with("flags")).map(str::to_string))
        .unwrap_or_default();
    let has = |f: &str| flags.split_whitespace().any(|x| x == f);
    let fast = Path::new(WHISPER_DIR).join("zohara-whisper-avx2");
    let base = Path::new(WHISPER_DIR).join("zohara-whisper");
    if ["avx", "avx2", "bmi2", "fma", "f16c"].iter().all(|f| has(*f)) && fast.exists() {
        return Some(fast);
    }
    base.exists().then_some(base)
}

/// Why the microphone button cannot work here, if it cannot.
pub fn missing() -> Option<String> {
    if engine().is_none() || !Path::new(MODEL).exists() {
        return Some("The Zohara speech engine is not installed (packages zohara-voice and zohara-voice-model).".into());
    }
    if !std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join("parecord").is_file())).unwrap_or(false) {
        return Some("The audio recording tool parecord (libpulse) is not installed.".into());
    }
    None
}

/// The language chosen for voice typing in Zohara Settings ("auto" when none).
fn language() -> String {
    Command::new("kreadconfig6")
        .args(["--file", "zoharadictationrc", "--group", "General", "--key", "Language"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "auto".into())
}

pub struct Recording {
    child: Child,
    samples: Arc<Mutex<Vec<i16>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Recording {
    pub fn start() -> Result<Recording, String> {
        if let Some(m) = missing() {
            return Err(m);
        }
        let mut child = Command::new("parecord")
            .args(["--raw", "--rate=16000", "--channels=1", "--format=s16le", "--latency-msec=30",
                "--client-name=Zohara Agent", "--stream-name=Voice to text"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start recording: {e}"))?;
        let mut out = child.stdout.take().ok_or("Could not read from the microphone")?;
        let samples = Arc::new(Mutex::new(Vec::with_capacity(RATE * 10)));
        let s2 = samples.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 960];
            while out.read_exact(&mut buf).is_ok() {
                let mut s = s2.lock().unwrap();
                s.extend(buf.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
                if s.len() >= RATE * MAX_SECONDS {
                    break;
                }
            }
        });
        Ok(Recording { child, samples, reader: Some(reader) })
    }

    /// Stops recording and turns what was said into text (slow: run it off the interface thread).
    pub fn finish(mut self) -> Result<String, String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let samples = self.samples.lock().unwrap().clone();
        if samples.len() < RATE {
            return Err("Nothing was recorded (whisper needs at least a second).".into());
        }
        let dir = std::env::temp_dir().join(format!("zohara-agent-voice-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let wav = dir.join("speech.wav");
        let result = write_wav(&wav, &samples).map_err(|e| e.to_string()).and_then(|_| transcribe(&wav));
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// Stops and throws the recording away.
    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn write_wav(path: &Path, s: &[i16]) -> std::io::Result<()> {
    let data_len = (s.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(RATE as u32).to_le_bytes());
    b.extend_from_slice(&(RATE as u32 * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in s {
        b.extend_from_slice(&x.to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Joins whisper's lines and drops its non-speech markers such as `[BLANK_AUDIO]` or `(music)`.
pub fn clean(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for ch in raw.chars() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn transcribe(wav: &Path) -> Result<String, String> {
    let bin = engine().ok_or("The speech engine is not installed")?;
    let threads = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4).to_string();
    let o = Command::new(&bin)
        .args(["-m", MODEL, "-l", &language(), "-t", &threads, "-nt", "-np", "-f"])
        .arg(wav)
        .output()
        .map_err(|e| format!("Could not start the speech engine: {e}"))?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        return Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Speech recognition failed").to_string());
    }
    let text = clean(&String::from_utf8_lossy(&o.stdout));
    if text.is_empty() {
        Err("No speech was heard.".into())
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_dropped() {
        assert_eq!(clean(" [BLANK_AUDIO]\n hello  (music) there "), "hello there");
    }

    #[test]
    fn a_wav_file_has_a_valid_header() {
        let p = std::env::temp_dir().join(format!("zo-wav-test-{}.wav", std::process::id()));
        write_wav(&p, &[1, 2, 3]).unwrap();
        let b = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_file(&p);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(b.len(), 44 + 6);
    }
}
