//! ALICE-Browser × ALICE-Voice bridge
//!
//! In-browser voice: Web Audio API PCM capture → ALICE-Voice LPC codec playback.
//!
//! Author: Moroya Sakamoto

/// Web audio capture configuration
#[derive(Debug, Clone)]
pub struct WebAudioConfig {
    pub sample_rate: u32,
    pub channels: u8,
    pub buffer_size: usize,
}

impl Default for WebAudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 4096,
        }
    }
}

/// Voice activity detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceActivity {
    Silent,
    Speech,
    Music,
}

/// Simple energy-based voice activity detector
pub fn detect_voice_activity(samples: &[f32], threshold_db: f32) -> VoiceActivity {
    if samples.is_empty() { return VoiceActivity::Silent; }

    let energy = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let energy_db = if energy > 1e-10 { 10.0 * energy.log10() } else { -100.0 };

    if energy_db < threshold_db {
        return VoiceActivity::Silent;
    }

    // Zero-crossing rate to distinguish speech vs music
    let mut zcr = 0u32;
    for i in 1..samples.len() {
        if (samples[i] >= 0.0) != (samples[i - 1] >= 0.0) {
            zcr += 1;
        }
    }
    let zcr_rate = zcr as f32 / samples.len() as f32;

    if zcr_rate > 0.1 {
        VoiceActivity::Speech
    } else {
        VoiceActivity::Music
    }
}

/// Downsample from web audio rate to voice codec rate
pub fn downsample_to_16k(samples: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate <= 16000 { return samples.to_vec(); }
    let ratio = src_rate as f32 / 16000.0;
    let out_len = (samples.len() as f32 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = (i as f32 * ratio) as usize;
            samples.get(src_idx).copied().unwrap_or(0.0)
        })
        .collect()
}

/// Browser voice session
pub struct BrowserVoiceSession {
    pub config: WebAudioConfig,
    pub frames_captured: u64,
    pub speech_frames: u64,
}

impl BrowserVoiceSession {
    pub fn new(config: WebAudioConfig) -> Self {
        Self { config, frames_captured: 0, speech_frames: 0 }
    }

    /// Process a captured audio buffer
    pub fn process_frame(&mut self, samples: &[f32]) -> VoiceActivity {
        self.frames_captured += 1;
        let activity = detect_voice_activity(samples, -40.0);
        if activity == VoiceActivity::Speech {
            self.speech_frames += 1;
        }
        activity
    }

    /// Speech ratio (0.0 - 1.0)
    pub fn speech_ratio(&self) -> f32 {
        if self.frames_captured == 0 { return 0.0; }
        self.speech_frames as f32 / self.frames_captured as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_silence() {
        let silence = vec![0.0f32; 1024];
        assert_eq!(detect_voice_activity(&silence, -40.0), VoiceActivity::Silent);
    }

    #[test]
    fn test_detect_speech() {
        // High ZCR signal simulating speech
        let speech: Vec<f32> = (0..1024).map(|i| {
            if i % 3 == 0 { 0.5 } else { -0.5 }
        }).collect();
        assert_eq!(detect_voice_activity(&speech, -40.0), VoiceActivity::Speech);
    }

    #[test]
    fn test_downsample() {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let downsampled = downsample_to_16k(&samples, 48000);
        assert_eq!(downsampled.len(), 1600);
    }

    #[test]
    fn test_downsample_no_op() {
        let samples = vec![1.0f32; 100];
        let result = downsample_to_16k(&samples, 16000);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_voice_session() {
        let mut session = BrowserVoiceSession::new(WebAudioConfig::default());
        let silence = vec![0.0f32; 1024];
        session.process_frame(&silence);
        assert_eq!(session.frames_captured, 1);
        assert_eq!(session.speech_ratio(), 0.0);
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(detect_voice_activity(&[], -40.0), VoiceActivity::Silent);
    }
}
