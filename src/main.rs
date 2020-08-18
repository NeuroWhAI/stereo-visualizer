use ggez::{
    audio::{self, SoundSource},
    conf::{WindowSetup, WindowMode},
    error::GameError,
    event, graphics,
    input::keyboard,
    graphics::DrawParam,
    Context, GameResult,
};
use rodio::Source;
use rustfft::{num_complex::Complex, num_traits::Zero, FFTplanner, FFT};
use std::{fs::File, i16, io::BufReader, path, sync::Arc, env};

#[derive(Debug, Clone, Copy)]
struct DirectionalSource {
    dir: f32,
    amp: f32,
}

impl DirectionalSource {
    fn new() -> Self {
        DirectionalSource {
            dir: 0.0,
            amp: 0.0,
        }
    }
}

struct MainState {
    canvas_width: f32,
    canvas_height: f32,
    sound: Option<audio::Source>,
    sample_rate: u32,
    left_wave: Vec<f32>,
    right_wave: Vec<f32>,
    fft: Arc<dyn FFT<f32>>,
    left_fft: Vec<Complex<f32>>,
    right_fft: Vec<Complex<f32>>,
    left_rev: Vec<f32>,
    right_rev: Vec<f32>,
    directions: Vec<DirectionalSource>,
}

impl MainState {
    fn new(width: f32, height: f32) -> GameResult<Self> {
        let fft_size = 1024;

        let mut left_fft = Vec::with_capacity(fft_size);
        left_fft.resize(fft_size, Complex::zero());

        let mut right_fft = Vec::with_capacity(fft_size);
        right_fft.resize(fft_size, Complex::zero());

        let mut left_rev = Vec::with_capacity(fft_size / 2);
        left_rev.resize(left_rev.capacity(), 0.0);

        let mut right_rev = Vec::with_capacity(fft_size / 2);
        right_rev.resize(right_rev.capacity(), 0.0);

        let mut directions = Vec::with_capacity(fft_size / 2);
        directions.resize(directions.capacity(), DirectionalSource::new());

        Ok(MainState {
            canvas_width: width,
            canvas_height: height,
            sound: None,
            sample_rate: 0,
            left_wave: Vec::new(),
            right_wave: Vec::new(),
            fft: FFTplanner::new(false).plan_fft(fft_size),
            left_fft,
            right_fft,
            left_rev,
            right_rev,
            directions,
        })
    }

    fn load_sound<P>(&mut self, path: P, ctx: &mut Context) -> GameResult
    where
        P: AsRef<path::Path>,
    {
        self.left_wave.clear();
        self.right_wave.clear();
        self.sound = None;

        let mut sound = audio::Source::new(ctx, path::Path::new("/").join(&path))?;
        sound.set_volume(0.4);
        self.sound = Some(sound);

        let source = File::open(path)
            .map_err(|err| err.to_string())
            .and_then(|file| {
                rodio::Decoder::new(BufReader::new(file))
                    .map_err(|err| err.to_string())
            });

        match source {
            Ok(source) if source.channels() == 2 => {
                self.sample_rate = source.sample_rate();
                dbg!(self.sample_rate);

                let samples: Vec<_> = source.collect();
                self.left_wave = samples
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, &amp)| if idx % 2 == 0 { Some(amp) } else { None })
                    .map(|amp| amp as f32 / i16::MAX as f32)
                    .collect();
                self.right_wave = samples
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, &amp)| if idx % 2 != 0 { Some(amp) } else { None })
                    .map(|amp| amp as f32 / i16::MAX as f32)
                    .collect();

                dbg!(self.left_wave.len());
                dbg!(self.right_wave.len());

                Ok(())
            }
            Ok(_) => Err(GameError::AudioError("Channels must be stereo".into())),
            Err(err) => Err(GameError::FilesystemError(err)),
        }
    }

    fn toggle_sound(&mut self) {
        if let Some(ref mut sound) = self.sound {
            if sound.playing() {
                sound.pause();
            } else if sound.stopped() {
                sound.play().expect("Play stopped sound");
            } else {
                sound.resume();
            }
        }
    }
}

impl event::EventHandler for MainState {
    fn update(&mut self, _ctx: &mut Context) -> GameResult {
        if let Some(ref sound) = self.sound {
            if sound.playing() {
                let time = sound.elapsed().as_secs_f32();
                let offset = (time * self.sample_rate as f32).floor() as usize;

                if offset + self.left_fft.len() <= self.left_wave.len()
                    && offset + self.right_fft.len() <= self.right_wave.len()
                {
                    let mut left_input: Vec<_> = (&self.left_wave
                        [offset..offset + self.left_fft.len()])
                        .into_iter()
                        .map(|&amp| Complex::new(amp, 0.0))
                        .collect();
                    self.fft.process(left_input.as_mut_slice(), self.left_fft.as_mut_slice());

                    let mut right_input: Vec<_> = (&self.right_wave
                        [offset..offset + self.right_fft.len()])
                        .into_iter()
                        .map(|&amp| Complex::new(amp, 0.0))
                        .collect();
                    self.fft.process(right_input.as_mut_slice(), self.right_fft.as_mut_slice());

                    for idx in 0..self.directions.len() {
                        let source = &mut self.directions[idx];

                        let left_amp = self.left_fft[idx].re.abs();
                        let right_amp = self.right_fft[idx].re.abs();

                        self.left_rev[idx] += (left_amp - self.left_rev[idx]) * 0.9;
                        self.right_rev[idx] += (right_amp - self.right_rev[idx]) * 0.9;

                        source.amp = self.left_rev[idx].max(self.right_rev[idx]);
                        source.dir = (self.right_rev[idx] - self.left_rev[idx]) / source.amp.max(1.0);
                    }
                }
            }
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        graphics::clear(ctx, [0.0, 0.0, 0.0, 1.0].into());

        let padding = 64.0;

        let bass = self.directions.iter()
            .skip(1)
            .take(4)
            .fold(0.0, |acc, source| acc + source.amp * 0.08) / 32.0;
        if bass > 0.0 {
            let max_height = 96.0;
            let height = (bass * max_height).min(max_height);
            let alpha = (height / max_height * 255.0).min(255.0).floor() as u8;
            let rect = graphics::Rect::new(0.0, (self.canvas_height - height) / 2.0, self.canvas_width, height);
            let mesh = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                rect,
                graphics::Color::from_rgba(30, 30, 30, alpha),
            )?;
            graphics::draw(ctx, &mesh, DrawParam::default())?;
        }

        for idx in 32..self.directions.len() {
            let source = &self.directions[idx];

            let alpha = (source.amp * 0.08 * 255.0).min(255.0).floor() as u8;

            if alpha < 8 {
                continue;
            }

            let width = source.amp * 0.5;
            let height = self.canvas_height / 5.0 + source.amp * 8.0;

            let x = (source.dir + 1.0) / 2.0;
            let x = padding + x * (self.canvas_width - padding * 2.0);

            let y = self.canvas_height / 2.0;

            let freq = (idx as f32 / self.directions.len() as f32 * 255.0).floor() as u8;

            let rect = graphics::Rect::new(x - width / 2.0, y - height / 2.0, width, height);
            let mesh = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                rect,
                graphics::Color::from_rgba(freq, 128, 192, alpha),
            )?;
            graphics::draw(ctx, &mesh, DrawParam::default())?;
        }

        graphics::present(ctx)?;
        Ok(())
    }

    fn key_down_event(
        &mut self,
        ctx: &mut Context,
        keycode: keyboard::KeyCode,
        _keymod: keyboard::KeyMods,
        _repeat: bool,
    ) {
        match keycode {
            keyboard::KeyCode::Space => self.toggle_sound(),
            keyboard::KeyCode::Escape => event::quit(ctx),
            _ => (),
        }
    }
}

fn main() -> GameResult {
    let args: Vec<String> = env::args().skip(1).collect();

    let width = 1024.0;
    let height = 768.0;

    let win_setup = WindowSetup::default()
        .title("Stereo Visualizer");
    let win_mode = WindowMode::default()
        .dimensions(width, height);

    let cb = ggez::ContextBuilder::new("stereo-visualizer", "neurowhai")
        .window_setup(win_setup)
        .window_mode(win_mode)
        .add_resource_path(path::PathBuf::from("."));
    let (ctx, event_loop) = &mut cb.build()?;

    let state = &mut MainState::new(width, height)?;
    
    if args.len() == 1 {
        state.load_sound(&args[0], ctx)?;
    }
    else {
        state.load_sound("sound.mp3", ctx)?;
    }

    println!("Ready");

    event::run(ctx, event_loop, state)
}
