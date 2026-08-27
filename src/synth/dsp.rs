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
