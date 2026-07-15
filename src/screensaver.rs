use std::time::Instant;

use rand::Rng;

use crate::config::ScreensaverConfig;

pub type Rgb = [f64; 3];

#[derive(Clone)]
pub struct Theme {
    pub palette: Vec<Rgb>,
    pub dim: Vec<Rgb>,
    pub bright: Rgb,
}

fn themes(name: &str) -> Theme {
    match name {
        "nord" => Theme {
            palette: vec![[136.0, 192.0, 208.0], [129.0, 161.0, 193.0], [143.0, 188.0, 187.0], [163.0, 190.0, 140.0]],
            dim: vec![[46.0, 52.0, 64.0], [59.0, 66.0, 82.0], [67.0, 76.0, 94.0], [76.0, 86.0, 106.0]],
            bright: [236.0, 239.0, 244.0],
        },
        "dracula" => Theme {
            palette: vec![[189.0, 147.0, 249.0], [255.0, 121.0, 198.0], [139.0, 233.0, 253.0], [80.0, 250.0, 123.0]],
            dim: vec![[48.0, 34.0, 78.0], [68.0, 28.0, 52.0], [32.0, 58.0, 68.0], [18.0, 62.0, 32.0]],
            bright: [248.0, 248.0, 242.0],
        },
        "gruvbox" => Theme {
            palette: vec![[251.0, 189.0, 35.0], [184.0, 187.0, 38.0], [214.0, 93.0, 14.0], [104.0, 157.0, 106.0]],
            dim: vec![[58.0, 44.0, 8.0], [44.0, 46.0, 10.0], [50.0, 22.0, 4.0], [26.0, 38.0, 24.0]],
            bright: [235.0, 219.0, 178.0],
        },
        "forest" => Theme {
            palette: vec![[80.0, 200.0, 90.0], [60.0, 160.0, 100.0], [160.0, 220.0, 80.0], [40.0, 180.0, 140.0]],
            dim: vec![[14.0, 38.0, 16.0], [12.0, 30.0, 20.0], [33.0, 48.0, 14.0], [10.0, 38.0, 28.0]],
            bright: [200.0, 240.0, 180.0],
        },
        "mono" => Theme {
            palette: vec![[0.0, 200.0, 80.0], [0.0, 170.0, 65.0], [0.0, 230.0, 95.0], [0.0, 150.0, 55.0]],
            dim: vec![[0.0, 38.0, 14.0], [0.0, 28.0, 11.0], [0.0, 46.0, 16.0], [0.0, 24.0, 9.0]],
            bright: [180.0, 255.0, 200.0],
        },
        _ => Theme {
            palette: vec![[100.0, 140.0, 230.0], [160.0, 100.0, 220.0], [80.0, 200.0, 220.0], [180.0, 140.0, 255.0]],
            dim: vec![[25.0, 35.0, 70.0], [40.0, 22.0, 60.0], [18.0, 50.0, 60.0], [45.0, 30.0, 70.0]],
            bright: [230.0, 235.0, 255.0],
        },
    }
}

fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] * (1.0 - t) + b[0] * t,
        a[1] * (1.0 - t) + b[1] * t,
        a[2] * (1.0 - t) + b[2] * t,
    ]
}

pub struct FrameBuf {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Option<(char, Rgb)>>,
}

impl FrameBuf {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, cells: vec![None; w * h] }
    }

    fn clear(&mut self) {
        self.cells.fill(None);
    }

    fn set(&mut self, x: i64, y: i64, ch: char, color: Rgb) {
        if x < 0 || x >= self.w as i64 || y < 0 || y >= self.h as i64 {
            return;
        }
        self.cells[y as usize * self.w + x as usize] = Some((ch, color));
    }
}

trait Scene {
    fn init(&mut self, w: usize, h: usize, theme: Theme);
    fn update(&mut self, dt: f64);
    fn draw(&self, buf: &mut FrameBuf);
}

// ---------------------------------------------------------------------------
// Starfield

const SF_GLYPHS: [char; 4] = ['·', '∘', '*', '✦'];

struct Star {
    x: f64,
    y: f64,
    z: f64,
    prev_px: i64,
    prev_py: i64,
    has_prev: bool,
    pal_idx: usize,
}

#[derive(Default)]
struct Starfield {
    w: usize,
    h: usize,
    theme: Option<Theme>,
    stars: Vec<Star>,
}

impl Starfield {
    fn spawn(&self, scattered: bool) -> Star {
        let mut rng = rand::thread_rng();
        let palette_len = self.theme.as_ref().map(|t| t.palette.len()).unwrap_or(1);
        Star {
            x: rng.gen::<f64>() * 2.0 - 1.0,
            y: rng.gen::<f64>() * 2.0 - 1.0,
            z: if scattered {
                0.02 + rng.gen::<f64>() * 0.98
            } else {
                0.75 + rng.gen::<f64>() * 0.25
            },
            prev_px: 0,
            prev_py: 0,
            has_prev: false,
            pal_idx: rng.gen_range(0..palette_len),
        }
    }

    fn project(&self, st: &Star) -> (i64, i64, bool) {
        let px = (self.w as f64 * 0.5 + (st.x / st.z) * self.w as f64 * 0.5).round() as i64;
        let py = (self.h as f64 * 0.5 + (st.y / st.z) * self.h as f64 * 0.5).round() as i64;
        let ok = px >= 0 && px < self.w as i64 && py >= 0 && py < self.h as i64;
        (px, py, ok)
    }
}

impl Scene for Starfield {
    fn init(&mut self, w: usize, h: usize, theme: Theme) {
        self.w = w;
        self.h = h;
        self.theme = Some(theme);
        self.stars.clear();
        for _ in 0..160 {
            let s = self.spawn(true);
            self.stars.push(s);
        }
    }

    fn update(&mut self, dt: f64) {
        let speed = 0.3;
        for i in 0..self.stars.len() {
            let (px, py, ok) = self.project(&self.stars[i]);
            {
                let st = &mut self.stars[i];
                if ok {
                    st.prev_px = px;
                    st.prev_py = py;
                    st.has_prev = true;
                } else {
                    st.has_prev = false;
                }
                st.z -= speed * dt;
            }
            let expired = self.stars[i].z <= 0.01 || !self.project(&self.stars[i]).2;
            if expired {
                self.stars[i] = self.spawn(false);
            }
        }
    }

    fn draw(&self, buf: &mut FrameBuf) {
        let Some(theme) = &self.theme else { return };
        for st in &self.stars {
            let (px, py, ok) = self.project(st);
            if !ok {
                continue;
            }
            let b = (1.0 - st.z).powf(1.5);
            let pal = theme.palette[st.pal_idx % theme.palette.len()];
            let dm = theme.dim[st.pal_idx % theme.dim.len()];

            if st.has_prev && (st.prev_px != px || st.prev_py != py) {
                buf.set(st.prev_px, st.prev_py, '·', lerp(dm, pal, b * 0.35));
            }

            let gi = ((b * SF_GLYPHS.len() as f64).floor() as usize).min(SF_GLYPHS.len() - 1);
            let color = if b > 0.85 {
                lerp(pal, theme.bright, (b - 0.85) / 0.15)
            } else {
                lerp(dm, pal, b / 0.85)
            };
            buf.set(px, py, SF_GLYPHS[gi], color);
        }
    }
}

// ---------------------------------------------------------------------------
// Rain

const RAIN_CHARSET: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉ0123456789";

struct Drop {
    col: usize,
    y: f64,
    speed: f64,
    head_char: char,
    frame_age: u32,
}

#[derive(Default)]
struct Rain {
    w: usize,
    h: usize,
    theme: Option<Theme>,
    grid: Vec<f64>,
    drops: Vec<Drop>,
}

impl Rain {
    fn new_drop(&self, scattered: bool) -> Drop {
        let mut rng = rand::thread_rng();
        let chars: Vec<char> = RAIN_CHARSET.chars().collect();
        Drop {
            col: rng.gen_range(0..self.w.max(1)),
            y: if scattered { rng.gen::<f64>() * self.h as f64 } else { 0.0 },
            speed: 5.0 + rng.gen::<f64>() * 12.0,
            head_char: chars[rng.gen_range(0..chars.len())],
            frame_age: 0,
        }
    }
}

impl Scene for Rain {
    fn init(&mut self, w: usize, h: usize, theme: Theme) {
        self.w = w;
        self.h = h;
        self.theme = Some(theme);
        self.grid = vec![0.0; w * h];
        self.drops.clear();
        let count = ((w as f64 * 0.4) / 1.2).floor() as usize + 5;
        for _ in 0..count {
            let d = self.new_drop(true);
            self.drops.push(d);
        }
    }

    fn update(&mut self, dt: f64) {
        let decay = dt * 3.5;
        for v in self.grid.iter_mut() {
            *v = (*v - decay).max(0.0);
        }

        let trail_len = 14i64;
        let mut rng = rand::thread_rng();
        let chars: Vec<char> = RAIN_CHARSET.chars().collect();
        for i in 0..self.drops.len() {
            {
                let d = &mut self.drops[i];
                d.y += d.speed * dt;
            }
            let head_y = self.drops[i].y.floor() as i64;
            let col = self.drops[i].col;
            for t in 0..trail_len {
                let cy = head_y - t;
                if cy >= 0 && cy < self.h as i64 {
                    let b = 1.0 - t as f64 / trail_len as f64;
                    let idx = cy as usize * self.w + col;
                    if b > self.grid[idx] {
                        self.grid[idx] = b;
                    }
                }
            }
            {
                let d = &mut self.drops[i];
                d.frame_age += 1;
                if d.frame_age >= 3 + rng.gen_range(0..4) {
                    d.head_char = chars[rng.gen_range(0..chars.len())];
                    d.frame_age = 0;
                }
            }
            if self.drops[i].y > (self.h as i64 + trail_len) as f64 {
                self.drops[i] = self.new_drop(false);
            }
        }
    }

    fn draw(&self, buf: &mut FrameBuf) {
        let Some(theme) = &self.theme else { return };
        for y in 0..self.h {
            for x in 0..self.w {
                let b = self.grid[y * self.w + x];
                if b < 0.04 {
                    continue;
                }
                let p_idx = x % theme.palette.len();
                let (ch, color) = if b > 0.85 {
                    ('│', lerp(theme.palette[p_idx], theme.bright, (b - 0.85) / 0.15))
                } else if b > 0.55 {
                    ('╎', theme.palette[p_idx])
                } else if b > 0.3 {
                    ('╌', lerp(theme.dim[p_idx], theme.palette[p_idx], (b - 0.3) / 0.25))
                } else {
                    ('·', lerp(theme.dim[p_idx], theme.palette[p_idx], b / 0.3))
                };
                buf.set(x as i64, y as i64, ch, color);
            }
        }
        for d in &self.drops {
            let hy = d.y.floor() as i64;
            if hy >= 0 && hy < self.h as i64 {
                buf.set(d.col as i64, hy, d.head_char, theme.bright);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Particles

const P_GLYPHS: [char; 7] = ['◦', '·', '○', '•', '.', '°', '∘'];

fn flow_field(x: f64, y: f64, t: f64) -> (f64, f64) {
    (
        (x * 0.04 + t * 0.25).sin() * (y * 0.06 + t * 0.18).cos() * 0.6,
        (x * 0.06 + t * 0.2).cos() * (y * 0.04 + t * 0.22).sin() * 0.6,
    )
}

struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    glyph: char,
    pal_idx: usize,
    phase: f64,
}

#[derive(Default)]
struct Particles {
    w: usize,
    h: usize,
    theme: Option<Theme>,
    time: f64,
    trail: Vec<f64>,
    particles: Vec<Particle>,
}

impl Particles {
    fn new_particle(&self, scattered: bool) -> Particle {
        let mut rng = rand::thread_rng();
        let speed = 0.4 + rng.gen::<f64>() * 2.2;
        let angle = rng.gen::<f64>() * 2.0 * std::f64::consts::PI;
        let (x, y) = if scattered {
            (rng.gen::<f64>() * self.w as f64, rng.gen::<f64>() * self.h as f64)
        } else {
            match rng.gen_range(0..4) {
                0 => (rng.gen::<f64>() * self.w as f64, 0.0),
                1 => (rng.gen::<f64>() * self.w as f64, (self.h - 1) as f64),
                2 => (0.0, rng.gen::<f64>() * self.h as f64),
                _ => ((self.w - 1) as f64, rng.gen::<f64>() * self.h as f64),
            }
        };
        let palette_len = self.theme.as_ref().map(|t| t.palette.len()).unwrap_or(1);
        Particle {
            x,
            y,
            vx: angle.cos() * speed,
            vy: angle.sin() * speed,
            glyph: P_GLYPHS[rng.gen_range(0..P_GLYPHS.len())],
            pal_idx: rng.gen_range(0..palette_len),
            phase: rng.gen::<f64>() * 2.0 * std::f64::consts::PI,
        }
    }
}

impl Scene for Particles {
    fn init(&mut self, w: usize, h: usize, theme: Theme) {
        self.w = w;
        self.h = h;
        self.theme = Some(theme);
        self.time = 0.0;
        self.trail = vec![0.0; w * h];
        self.particles.clear();
        for _ in 0..120 {
            let p = self.new_particle(true);
            self.particles.push(p);
        }
    }

    fn update(&mut self, dt: f64) {
        self.time += dt;
        let decay = dt * 2.8;
        for v in self.trail.iter_mut() {
            *v = (*v - decay).max(0.0);
        }

        let friction = 0.98f64.powf(dt * 60.0);
        for i in 0..self.particles.len() {
            {
                let p = &mut self.particles[i];
                let (fx, fy) = flow_field(p.x, p.y, self.time);
                p.vx = (p.vx + fx * dt) * friction;
                p.vy = (p.vy + fy * dt) * friction;

                let speed = (p.vx * p.vx + p.vy * p.vy).sqrt();
                if speed > 3.0 {
                    p.vx = (p.vx / speed) * 3.0;
                    p.vy = (p.vy / speed) * 3.0;
                }

                p.x += p.vx * dt;
                p.y += p.vy * dt;
                p.phase += dt * 1.2;
            }

            let (ix, iy) = (self.particles[i].x.round() as i64, self.particles[i].y.round() as i64);
            if ix >= 0 && ix < self.w as i64 && iy >= 0 && iy < self.h as i64 {
                let idx = iy as usize * self.w + ix as usize;
                if self.trail[idx] < 0.9 {
                    self.trail[idx] = 0.9;
                }
            }

            let p = &self.particles[i];
            if p.x < -2.0 || p.x > self.w as f64 + 2.0 || p.y < -2.0 || p.y > self.h as f64 + 2.0 {
                self.particles[i] = self.new_particle(false);
            }
        }
    }

    fn draw(&self, buf: &mut FrameBuf) {
        let Some(theme) = &self.theme else { return };
        for y in 0..self.h {
            for x in 0..self.w {
                let b = self.trail[y * self.w + x];
                if b < 0.08 {
                    continue;
                }
                let p_idx = (x + y) % theme.dim.len();
                buf.set(x as i64, y as i64, '·', lerp(theme.dim[p_idx], theme.palette[p_idx], b * 0.45));
            }
        }
        for p in &self.particles {
            let (x, y) = (p.x.round() as i64, p.y.round() as i64);
            if x < 0 || x >= self.w as i64 || y < 0 || y >= self.h as i64 {
                continue;
            }
            let shimmer = 0.65 + 0.35 * p.phase.sin();
            buf.set(x, y, p.glyph, lerp(theme.palette[p.pal_idx], theme.bright, shimmer * 0.5));
        }
    }
}

// ---------------------------------------------------------------------------
// Engine

pub const SCENE_NAMES: [&str; 3] = ["starfield", "rain", "particles"];

fn make_scene(name: &str) -> Box<dyn Scene> {
    match name {
        "rain" => Box::new(Rain::default()),
        "particles" => Box::new(Particles::default()),
        _ => Box::new(Starfield::default()),
    }
}

struct Opts {
    enabled: bool,
    idle_seconds: f64,
    cycle_seconds: f64,
    fade_seconds: f64,
    theme: String,
    scenes: String,
    wake_on_log: bool,
}

enum Phase {
    Out,
    In,
}

pub struct Engine {
    opts: Opts,
    scene_names: Vec<String>,
    cur: usize,
    scenes: Vec<Box<dyn Scene>>,
    pub buf: FrameBuf,
    active: bool,
    last_activity: Instant,
    last_tick: Instant,
    scene_age: f64,
    transition: Option<(Phase, f64)>,
    w: usize,
    h: usize,
}

impl Engine {
    pub fn new(config: Option<&ScreensaverConfig>) -> Self {
        let d = ScreensaverConfig::default();
        let c = config.unwrap_or(&d);
        let opts = Opts {
            enabled: c.enabled.unwrap_or(true),
            idle_seconds: c.idle_seconds.unwrap_or(90.0),
            cycle_seconds: c.cycle_seconds.unwrap_or(60.0),
            fade_seconds: c.fade_seconds.unwrap_or(0.6),
            theme: c.theme.clone().unwrap_or_else(|| "cosmic".to_string()),
            scenes: c.scenes.clone().unwrap_or_else(|| "all".to_string()),
            wake_on_log: c.wake_on_log.unwrap_or(true),
        };
        let mut e = Self {
            opts,
            scene_names: Vec::new(),
            cur: 0,
            scenes: Vec::new(),
            buf: FrameBuf::new(0, 0),
            active: false,
            last_activity: Instant::now(),
            last_tick: Instant::now(),
            scene_age: 0.0,
            transition: None,
            w: 0,
            h: 0,
        };
        e.build_scenes();
        e
    }

    fn build_scenes(&mut self) {
        let spec = self.opts.scenes.trim().to_lowercase();
        let mut names: Vec<String> = if spec.is_empty() || spec == "all" {
            SCENE_NAMES.iter().map(|s| s.to_string()).collect()
        } else {
            spec.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| SCENE_NAMES.contains(&s.as_str()))
                .collect()
        };
        if names.is_empty() {
            names = SCENE_NAMES.iter().map(|s| s.to_string()).collect();
        }
        let mut rng = rand::thread_rng();
        for i in (1..names.len()).rev() {
            let j = rng.gen_range(0..=i);
            names.swap(i, j);
        }
        self.scenes = names.iter().map(|n| make_scene(n)).collect();
        self.scene_names = names;
        self.cur = 0;
    }

    fn theme(&self) -> Theme {
        themes(&self.opts.theme)
    }

    fn init_scene(&mut self) {
        self.buf = FrameBuf::new(self.w, self.h);
        let theme = self.theme();
        let (w, h) = (self.w, self.h);
        self.scenes[self.cur].init(w, h, theme);
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self, scene_name: Option<&str>, w: usize, h: usize) {
        if self.active {
            return;
        }
        if let Some(name) = scene_name {
            if SCENE_NAMES.contains(&name) {
                self.scenes = vec![make_scene(name)];
                self.scene_names = vec![name.to_string()];
                self.cur = 0;
            }
        } else if self.scenes.len() <= 1 {
            self.build_scenes();
        }
        self.w = w;
        self.h = h;
        self.active = true;
        self.scene_age = 0.0;
        self.transition = None;
        self.init_scene();
        self.last_tick = Instant::now();
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
        if self.active {
            self.stop();
        }
    }

    pub fn log_activity(&mut self) {
        if self.opts.wake_on_log {
            self.touch();
        }
    }

    /// Retorna true quando a tecla deve ser engolida (usada só para acordar).
    pub fn handle_key(&mut self) -> bool {
        self.last_activity = Instant::now();
        if self.active {
            self.stop();
            return true;
        }
        false
    }

    pub fn check_idle(&mut self, can_start: bool, w: usize, h: usize) {
        if self.active || !self.opts.enabled {
            return;
        }
        if self.last_activity.elapsed().as_secs_f64() >= self.opts.idle_seconds && can_start {
            self.start(None, w, h);
        }
    }

    fn fade_alpha(&self) -> f64 {
        let dur = self.opts.fade_seconds;
        let Some((phase, t)) = &self.transition else {
            return 0.0;
        };
        if dur <= 0.0 {
            return 0.0;
        }
        match phase {
            Phase::Out => t / dur,
            Phase::In => 1.0 - t / dur,
        }
    }

    pub fn frame(&mut self, w: usize, h: usize) -> f64 {
        if w != self.w || h != self.h {
            self.w = w;
            self.h = h;
            self.init_scene();
        }

        let now = Instant::now();
        let mut dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if dt > 0.1 {
            dt = 0.1;
        }

        if let Some((phase, t)) = &mut self.transition {
            *t += dt;
            if *t >= self.opts.fade_seconds {
                match phase {
                    Phase::Out => {
                        self.cur = (self.cur + 1) % self.scenes.len();
                        self.scene_age = 0.0;
                        self.init_scene();
                        self.transition = Some((Phase::In, 0.0));
                    }
                    Phase::In => {
                        self.transition = None;
                    }
                }
            }
        }

        self.scenes[self.cur].update(dt);
        self.buf.clear();
        self.scenes[self.cur].draw(&mut self.buf);

        if self.opts.cycle_seconds > 0.0 && self.scenes.len() > 1 && self.transition.is_none() {
            self.scene_age += dt;
            if self.scene_age >= self.opts.cycle_seconds {
                if self.opts.fade_seconds > 0.0 {
                    self.transition = Some((Phase::Out, 0.0));
                } else {
                    self.cur = (self.cur + 1) % self.scenes.len();
                    self.scene_age = 0.0;
                    self.init_scene();
                }
            }
        }

        self.fade_alpha()
    }
}
