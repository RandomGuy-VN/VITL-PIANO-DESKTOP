/// Algorithmic Stereo Reverb (Freeverb / Schroeder architecture)
pub struct Reverb {
    comb_filters_l: Vec<CombFilter>,
    comb_filters_r: Vec<CombFilter>,
    allpass_filters_l: Vec<AllpassFilter>,
    allpass_filters_r: Vec<AllpassFilter>,
    pub wet_mix: f32,
    pub room_size: f32,
    pub damping: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let sr_scale = sample_rate / 44100.0;
        let comb_tunings = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let allpass_tunings = [556, 441, 341, 225];
        let stereo_spread = 23;

        let comb_filters_l = comb_tunings
            .iter()
            .map(|&t| CombFilter::new((t as f32 * sr_scale) as usize))
            .collect();

        let comb_filters_r = comb_tunings
            .iter()
            .map(|&t| CombFilter::new(((t + stereo_spread) as f32 * sr_scale) as usize))
            .collect();

        let allpass_filters_l = allpass_tunings
            .iter()
            .map(|&t| AllpassFilter::new((t as f32 * sr_scale) as usize))
            .collect();

        let allpass_filters_r = allpass_tunings
            .iter()
            .map(|&t| AllpassFilter::new(((t + stereo_spread) as f32 * sr_scale) as usize))
            .collect();

        Self {
            comb_filters_l,
            comb_filters_r,
            allpass_filters_l,
            allpass_filters_r,
            wet_mix: 0.3,
            room_size: 0.75,
            damping: 0.25,
        }
    }

    /// Process a stereo sample (left, right) in-place with reverb
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.wet_mix <= 0.001 {
            return (left, right);
        }

        let input = (left + right) * 0.5 * 0.015;
        let mut out_l = 0.0;
        let mut out_r = 0.0;

        for comb in &mut self.comb_filters_l {
            out_l += comb.process(input, self.room_size, self.damping);
        }

        for comb in &mut self.comb_filters_r {
            out_r += comb.process(input, self.room_size, self.damping);
        }

        for ap in &mut self.allpass_filters_l {
            out_l = ap.process(out_l);
        }

        for ap in &mut self.allpass_filters_r {
            out_r = ap.process(out_r);
        }

        let dry_mix = 1.0 - self.wet_mix;
        let final_l = left * dry_mix + out_l * self.wet_mix * 2.0;
        let final_r = right * dry_mix + out_r * self.wet_mix * 2.0;

        (final_l, final_r)
    }

    pub fn reset(&mut self) {
        for c in &mut self.comb_filters_l { c.reset(); }
        for c in &mut self.comb_filters_r { c.reset(); }
        for a in &mut self.allpass_filters_l { a.reset(); }
        for a in &mut self.allpass_filters_r { a.reset(); }
    }
}

struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    filter_state: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            index: 0,
            filter_state: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.index];
        self.filter_state = output * (1.0 - damping) + self.filter_state * damping;
        self.buffer[self.index] = input + self.filter_state * feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
        self.filter_state = 0.0;
    }
}

struct AllpassFilter {
    buffer: Vec<f32>,
    index: usize,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buffer[self.index];
        let output = -input + buf_out;
        self.buffer[self.index] = input + buf_out * 0.5;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }
}

/// Soft-knee limiter / saturation function to prevent digital clipping
#[inline(always)]
pub fn soft_limit(x: f32) -> f32 {
    if x.abs() < 0.8 {
        x
    } else {
        x / (1.0 + x * x).sqrt()
    }
}

/// 3-Band Equalizer (Low Shelf @ 100Hz, Mid Peak @ 1kHz, High Shelf @ 6kHz)
pub struct ThreeBandEqualizer {
    sample_rate: f32,
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
    // Filter states (L & R)
    low_l: BiquadState,
    low_r: BiquadState,
    mid_l: BiquadState,
    mid_r: BiquadState,
    high_l: BiquadState,
    high_r: BiquadState,
}

#[derive(Default, Clone)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl ThreeBandEqualizer {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(8000.0);
        Self {
            sample_rate,
            low_db: 0.0,
            mid_db: 0.0,
            high_db: 0.0,
            low_l: BiquadState::default(),
            low_r: BiquadState::default(),
            mid_l: BiquadState::default(),
            mid_r: BiquadState::default(),
            high_l: BiquadState::default(),
            high_r: BiquadState::default(),
        }
    }

    pub fn set_gains(&mut self, low_db: f32, mid_db: f32, high_db: f32) {
        self.low_db = low_db.clamp(-15.0, 15.0);
        self.mid_db = mid_db.clamp(-15.0, 15.0);
        self.high_db = high_db.clamp(-15.0, 15.0);
    }

    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        if self.low_db.abs() < 0.05 && self.mid_db.abs() < 0.05 && self.high_db.abs() < 0.05 {
            return (in_l, in_r);
        }

        // Low shelf coefficients
        let (b0_l, b1_l, b2_l, a1_l, a2_l) = calculate_low_shelf(120.0, self.low_db, self.sample_rate);
        let out_l1 = process_biquad(in_l, &mut self.low_l, b0_l, b1_l, b2_l, a1_l, a2_l);
        let out_r1 = process_biquad(in_r, &mut self.low_r, b0_l, b1_l, b2_l, a1_l, a2_l);

        // Mid peak coefficients
        let (b0_m, b1_m, b2_m, a1_m, a2_m) = calculate_peaking(1000.0, self.mid_db, 1.0, self.sample_rate);
        let out_l2 = process_biquad(out_l1, &mut self.mid_l, b0_m, b1_m, b2_m, a1_m, a2_m);
        let out_r2 = process_biquad(out_r1, &mut self.mid_r, b0_m, b1_m, b2_m, a1_m, a2_m);

        // High shelf coefficients
        let (b0_h, b1_h, b2_h, a1_h, a2_h) = calculate_high_shelf(5000.0, self.high_db, self.sample_rate);
        let out_l3 = process_biquad(out_l2, &mut self.high_l, b0_h, b1_h, b2_h, a1_h, a2_h);
        let out_r3 = process_biquad(out_r2, &mut self.high_r, b0_h, b1_h, b2_h, a1_h, a2_h);

        (out_l3, out_r3)
    }

    pub fn reset(&mut self) {
        self.low_l = BiquadState::default();
        self.low_r = BiquadState::default();
        self.mid_l = BiquadState::default();
        self.mid_r = BiquadState::default();
        self.high_l = BiquadState::default();
        self.high_r = BiquadState::default();
    }
}

#[inline(always)]
fn process_biquad(input: f32, s: &mut BiquadState, b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> f32 {
    let output = b0 * input + b1 * s.x1 + b2 * s.x2 - a1 * s.y1 - a2 * s.y2;
    s.x2 = s.x1;
    s.x1 = input;
    s.y2 = s.y1;
    s.y1 = output;
    output
}

fn calculate_low_shelf(fc: f32, gain_db: f32, sr: f32) -> (f32, f32, f32, f32, f32) {
    let a = 10.0f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * fc / sr;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0f32.sqrt();

    let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha;
    let b0 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha)) / a0;
    let b1 = (2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
    let b2 = (a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha)) / a0;
    let a1 = (-2.0 * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
    let a2 = ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha) / a0;

    (b0, b1, b2, a1, a2)
}

fn calculate_peaking(fc: f32, gain_db: f32, q: f32, sr: f32) -> (f32, f32, f32, f32, f32) {
    let a = 10.0f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * fc / sr;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);

    let a0 = 1.0 + alpha / a;
    let b0 = (1.0 + alpha * a) / a0;
    let b1 = (-2.0 * cos_w0) / a0;
    let b2 = (1.0 - alpha * a) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha / a) / a0;

    (b0, b1, b2, a1, a2)
}

fn calculate_high_shelf(fc: f32, gain_db: f32, sr: f32) -> (f32, f32, f32, f32, f32) {
    let a = 10.0f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * fc / sr;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / 2.0 * 2.0f32.sqrt();

    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha;
    let b0 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * a.sqrt() * alpha)) / a0;
    let b1 = (-2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0)) / a0;
    let b2 = (a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha)) / a0;
    let a1 = (2.0 * ((a - 1.0) - (a + 1.0) * cos_w0)) / a0;
    let a2 = ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * a.sqrt() * alpha) / a0;

    (b0, b1, b2, a1, a2)
}

/// Stereo Delay / Echo with damping & feedback
pub struct StereoDelay {
    sample_rate: f32,
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_idx: usize,
    pub delay_time_ms: f32,
    pub feedback: f32,
    pub wet_mix: f32,
    pub enabled: bool,
}

impl StereoDelay {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(8000.0);
        let max_samples = ((sample_rate * 2.0) as usize).max(4); // Max 2 seconds delay
        Self {
            sample_rate,
            buffer_l: vec![0.0; max_samples],
            buffer_r: vec![0.0; max_samples],
            write_idx: 0,
            delay_time_ms: 250.0,
            feedback: 0.35,
            wet_mix: 0.25,
            enabled: false,
        }
    }

    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        if !self.enabled || self.wet_mix <= 0.001 || self.buffer_l.len() < 2 {
            return (in_l, in_r);
        }

        let delay_samples = ((self.delay_time_ms.max(1.0) * 0.001 * self.sample_rate) as usize).clamp(1, self.buffer_l.len() - 1);
        let read_idx_l = (self.write_idx + self.buffer_l.len() - delay_samples) % self.buffer_l.len();
        // Ping-pong stereo offset (+20ms for right channel)
        let delay_samples_r = ((self.delay_time_ms * 0.001 * self.sample_rate * 1.08) as usize).clamp(1, self.buffer_r.len() - 1);
        let read_idx_r = (self.write_idx + self.buffer_r.len() - delay_samples_r) % self.buffer_r.len();

        let delayed_l = self.buffer_l[read_idx_l];
        let delayed_r = self.buffer_r[read_idx_r];

        self.buffer_l[self.write_idx] = in_l + delayed_r * self.feedback;
        self.buffer_r[self.write_idx] = in_r + delayed_l * self.feedback;

        self.write_idx = (self.write_idx + 1) % self.buffer_l.len();

        let dry_mix = 1.0 - self.wet_mix;
        (
            in_l * dry_mix + delayed_l * self.wet_mix,
            in_r * dry_mix + delayed_r * self.wet_mix,
        )
    }

    pub fn reset(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_idx = 0;
    }
}
