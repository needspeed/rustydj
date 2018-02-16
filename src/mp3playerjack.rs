extern crate simplemad;
extern crate cpal;
extern crate crossbeam;
extern crate jack;

use simplemad::{Decoder, Frame, MadFixed32};
use std::time::Duration;
use std::fs::File;
use std::path::Path;
use std::cell::Cell;
use std::sync::{mpsc};
use std::sync::mpsc::{SyncSender, Receiver};
use std::collections::HashMap;

use library::Track;

use jack::prelude::{AsyncClient, AudioOutPort, AudioOutSpec, Client, ClosureProcessHandler,
                    JackControl, ProcessScope, client_options, PortFlags, PortSpec};

#[derive(Debug, Deserialize)]
pub enum PlayerCommand {
    GetPos,
    Seek(f64),
    SeekS(Duration),
    PlayPause,
    Cue(bool),
    CueMove(bool),
    HotCue(usize,bool),
    Speed(f64),
    SpeedDiff(f64),
    Scratch(f64),
    Open(Track),
}

#[derive(Debug, Serialize)]
pub enum PlayerStatus {
    TrackInfo(Option<Track>, Duration, u32), //track, duration, sample_rate
    Pos(Duration, f64),
    Speed(f64),
    Print(String),
}

pub struct Mp3Player {
    _frames : Vec<Frame>,
    _out_port : Option<Box<jack::port::Port<jack::port::AudioOutSpec>>>,
    _client : Option<Box<jack::client::Client>>,
    txui: Option<mpsc::SyncSender<PlayerStatus>>,
}


const SAMPLE_SKIP : usize = 1;
const AUTO_PLAY : bool = false;
const PRINT : bool = true;

macro_rules! printinfo {
    ($self:ident, $($args:expr),*) => {{
        if PRINT {
            println!($($args),*);
        }
        else if let Some(ref txui_) = $self.txui {
           (*txui_).try_send(PlayerStatus::Print(format!($($args),*))).is_ok();
        }
    }};
}

impl Mp3Player {

    pub fn new() -> Mp3Player {
        Mp3Player {
            _frames : Vec::new(),
            _out_port : None,
            _client : None,
            txui : None,
        }
    }

    pub fn open(&mut self, filename : &str) -> bool {
        let path = Path::new(filename);
        printinfo!(self, "Playing: {}", filename);
        if let Ok(file) = File::open(&path) {
            if let Ok(decoder) = Decoder::decode(file) {
                printinfo!(self, "Loading...");
                self._frames = decoder
                    .filter_map(|r| match r {
                        Ok(f) => {
                            if f.samples.len() != 2 || f.samples[0].len() != f.samples[1].len() {
                                printinfo!(self, "Dropped invalid frame at ({:?})", f.position);
                                return None;
                            }
                            if f.samples[0].len() != 1152 {
                                printinfo!(self, "Warning frame with {} samples at ({:?}). Timing might be inaccurate", f.samples[0].len(), f.position);
                            }
                            Some(f)
                        },
                        Err(_) => None}).collect();
                true
            }
            else {
                printinfo!(self, "Could not decode file: {}", filename);
                false
            }
        }
        else {
            printinfo!(self, "Could not open file: {}", filename);
            false
        }
    }

    fn feed<'a>(&'a mut self, sink_sample_rate: u32, rx : Receiver<PlayerCommand>, tx : SyncSender<PlayerStatus>, t : SyncSender<(Vec<Vec<MadFixed32>>, f64, f64)>, rr : Receiver<f64>) {
        let mut cur_time = Duration::new(0, 0);
        let mut sample_rate = 0;
        let mut duration;
        let mut sample_time_nanos = 0.0;

        let mut empty_samples = Vec::new();
        empty_samples.push(vec![MadFixed32::from(0); 1]);
        empty_samples.push(vec![MadFixed32::from(0); 1]);

        let mut j : f64 = 0.0;
        let mut i : usize = 0;
        let mut playing = false;
        let mut cue_sample = 0.0;
        let mut hotcues : HashMap<usize, f64> = HashMap::new();
        let mut cue_markers = Vec::new();
        let mut speed_factor_bend = 1.0;
        let mut speed_factor_fader = 1.0;
        let mut speed_factor_resample = 1.0;
        let mut jumped = false;
        let mut loaded = false;
        let mut true_playing = false;

        macro_rules! cur_sample {
            () => { (i*1152) as f64 + j };
        }

        macro_rules! jump {
            ($new_pos:expr) => {{
                let mut new_pos_ = $new_pos;
                if new_pos_< 0.0 {
                    new_pos_ = 0.0;
                }
                i = (new_pos_ / 1152.0) as usize;
                j = (new_pos_ % 1152.0) as f64;
                jumped = true;
                if let Some(f) = self._frames.get(i) {
                    cur_time = f.position + Duration::from_millis(((sample_time_nanos as f64 * j) / 1000000.0) as u64);
                }
                tx.try_send(PlayerStatus::Pos(cur_time, cur_sample!())).is_ok();
            }};
        }

        macro_rules! set_play {
            ($play:expr, $is_true_play:expr) => {{
                let play_ = $play;
                //printinfo!(self, "{} {} | {} {} {}", play_, $is_true_play, playing, true_playing, loaded);
                if !(play_ && !loaded) {
                    if play_ && !playing {
                        t.send((empty_samples.clone(), 0.0, 1.0)).unwrap();
                    }
                    if !play_ && playing {
                        rr.recv().unwrap();
                        t.send((Vec::new(), 0.0, 1.0)).unwrap();
                    }
                    if !play_ && true_playing {
                        true_playing = false;
                    }
                    if play_ && !true_playing && $is_true_play {
                        true_playing = true;
                    }

                    playing = play_;
                }
                //printinfo!(self, "=> {} {} {}", playing, true_playing, loaded);
            }};
            ($play:expr) => {
                set_play!($play, false);
            };
        }
        
        loop {
            // MP3 feeder
            if let Ok(j_) = rr.try_recv() {
                if !jumped {
                    j = j_;
                    let ju = j as usize;
                    if j < 0.0 {
                        i -= 1;
                    }
                    if let Some(f) = self._frames.get(i) {
                        if j < 0.0 {
                            j = (f.samples[0].len()-1) as f64;
                        }
                        else if ju >= f.samples[0].len() {
                            j %= f.samples[0].len() as f64;
                            i += 1;
                        }
                        cur_time = f.position + Duration::from_millis(((sample_time_nanos as f64 * j) / 1000000.0) as u64);
                        tx.try_send(PlayerStatus::Pos(cur_time, cur_sample!())).is_ok();
                    }
                }
                if playing {
                    if let Some(f) = self._frames.get(i) {
                        assert_eq!(f.samples.len(), 2);
                        assert!(f.samples[0].len() > j as usize);
                        assert_eq!(f.samples[0].len(), f.samples[1].len());
                        //printinfo!(self, "Pos {:?} ({} {})", f.position, i, j);
                        let speed_factor = speed_factor_resample * speed_factor_fader * speed_factor_bend;
                        t.send((f.samples.clone(), j, speed_factor)).unwrap(); 
                        tx.try_send(PlayerStatus::Speed(speed_factor_fader*speed_factor_bend)).is_ok();
                        jumped = false;
                    }
                    else {
                        printinfo!(self, "Reached end");
                        playing = false;
                        loaded = false;
                        t.send((Vec::new(), 0.0, 1.0)).unwrap();
                    }
                }
            }
          
            // Command Handler
            if let Ok(cmd) = if playing { rx.try_recv() } else { Ok(rx.recv().unwrap()) } {
                match cmd {
                    PlayerCommand::GetPos => {tx.send(PlayerStatus::Pos(cur_time, cur_sample!())).unwrap(); ()},
                    PlayerCommand::Seek(new_pos) => jump!(new_pos),
                    PlayerCommand::SeekS(new_pos_) => jump!((new_pos_.as_secs() as u32* sample_rate) as f64),
                    PlayerCommand::PlayPause => set_play!(!true_playing, true),
                    PlayerCommand::Cue(on) => {
                        if on {
                            if cue_sample != cur_sample!() {
                                cue_sample = cur_sample!();
                                set_play!(false);
                            }
                            else {
                                set_play!(true);
                            }
                        }
                        else if !true_playing {
                            set_play!(false);
                            jump!(cue_sample);
                        }
                    },
                    PlayerCommand::CueMove(forward) => {
                        let cur_sample = cur_sample!();
                        let mut closest_in_direction = None;
                        for pos in cue_markers.iter() {
                            if ((forward && *pos > cur_sample) || (!forward && *pos < cur_sample)) 
                                && (closest_in_direction.is_none() || (forward && *pos < closest_in_direction.unwrap())
                                    || (!forward && *pos > closest_in_direction.unwrap())) {
                                closest_in_direction = Some(*pos);
                            }
                        }
                        if let Some(pos) = closest_in_direction {
                            jump!(pos);
                            set_play!(false);
                        }
                    },
                    PlayerCommand::HotCue(idx,on) => {
                        if hotcues.contains_key(&idx) {
                            let pos = hotcues[&idx];
                            if on {
                                jump!(pos);
                                set_play!(true);
                            }
                            else if !true_playing {
                                set_play!(false);
                                jump!(pos);
                            }
                        }
                        else if on {
                            hotcues.insert(idx, cur_sample!());
                        }
                    },
                    PlayerCommand::Speed(speed_factor) => {
                        speed_factor_fader = speed_factor;
                        tx.try_send(PlayerStatus::Speed(speed_factor_fader*speed_factor_bend)).is_ok();
                    },
                    PlayerCommand::Scratch(velocity) => { // -1 -> 1
                        if true_playing {
                            speed_factor_bend = if velocity < 0.0 {
                                (velocity + 2.0) / 2.0 //0.5 -> 1
                            }
                            else {
                                velocity + 1.0 //1.0 -> 2.0
                            }; // 0.5 -> 2.0
                        }
                        else {
                            if velocity != 0.0 {
                                let base : f64 = 4.0;
                                speed_factor_bend = base.powf(velocity.abs()*10.0)/10.0*velocity.signum();
                                //printinfo!(self, "{} -> {}", velocity, speed_factor_bend);
                            }
                            else {
                                speed_factor_bend = 1.0;
                                tx.try_send(PlayerStatus::Speed(speed_factor_fader*speed_factor_bend)).is_ok();
                            }
                            set_play!(velocity != 0.0, false);
                        }
                    }
                    PlayerCommand::SpeedDiff(speed_factor) => {
                        speed_factor_bend = speed_factor;
                        if !true_playing {
                            set_play!(speed_factor != 1.0, false);
                        }
                        if !playing {
                            tx.try_send(PlayerStatus::Speed(speed_factor_fader*speed_factor_bend)).is_ok();
                        }
                    }
                    PlayerCommand::Open(track) => {
                        let was_playing = playing;
                        let was_true_playing = true_playing;
                        set_play!(false);
                        if self.open(&*track.path) {
                            loaded = true;
                            sample_rate = self._frames[0].sample_rate;
                            speed_factor_resample = sample_rate as f64 / sink_sample_rate as f64;
                            printinfo!(self, "Resampling: {} -> {} ({}x)", sample_rate, sink_sample_rate, speed_factor_resample);
                            duration = self._frames.iter().map(|f| f.duration).fold(Duration::new(0, 0), |acc, dtn| acc + dtn);
                            printinfo!(self, "Start at: {:?}", self._frames[0].position);
                            sample_time_nanos = 1000000.0/(sample_rate as f64/1000.0);
                            cue_markers = track.cues.iter().map(|cue| cue.start as f64).collect();
                            hotcues = track.cues.iter().enumerate().map(|(idx, cue)| (idx, (cue.start as f64))).collect();
                            
                            set_play!(was_playing || AUTO_PLAY, was_true_playing || AUTO_PLAY);
                            jump!(track.first_beat as f64);
                            if let Some(cue_pos) = hotcues.get(&0) {
                                jump!(*cue_pos); 
                            }
                            tx.send(PlayerStatus::TrackInfo(Some(track), duration, sample_rate)).unwrap();
                        }
                    },
                    _ => (),
                };
            }
        }
    }

    pub fn play<'a>(&'a mut self, rx : Receiver<PlayerCommand>, tx : SyncSender<PlayerStatus>) {
        let (t, r) = mpsc::sync_channel::<(Vec<Vec<MadFixed32>>, f64, f64)>(0);
        let (tr, rr) = mpsc::sync_channel::<f64>(0);

        self.txui = Some(tx.clone());
        
        let client = Client::new("rustydj", client_options::NO_START_SERVER).unwrap().0;
        let mut l_chan = client.register_port("out_l", AudioOutSpec::default()).unwrap();
        let mut r_chan = client.register_port("out_r", AudioOutSpec::default()).unwrap();
        let port_names = [l_chan.name().to_string(), r_chan.name().to_string()];

        let playing = Cell::new(false);

        let process = ClosureProcessHandler::new(move |_client: &Client, ps: &ProcessScope| -> JackControl {
            let mut l_buffer = AudioOutPort::new(&mut l_chan, ps);
            let mut r_buffer = AudioOutPort::new(&mut r_chan, ps);

            let l_iter = l_buffer.iter_mut();
            let r_iter = r_buffer.iter_mut();
            let mut iter = l_iter.zip(r_iter);


            let mut samples = Vec::new(); 
            let mut s = 0.0;
            let mut speed_factor = 1.0;

            if playing.get() {
                let msg = r.recv().unwrap();
                samples = msg.0;
                s = msg.1;
                speed_factor = msg.2;
            }
            macro_rules! wait_on_pause {
                () => {
                    while samples.len() == 0 {
                        playing.set(false);
                        for _ in 0 .. SAMPLE_SKIP {
                            if let Some((l_elem, r_elem)) = iter.next() {
                                *l_elem = 0.0;
                                *r_elem = 0.0;
                            }
                            else {
                                return JackControl::Continue;
                            }
                        }
                        if let Ok((samples_, s_, speed_factor_)) = r.try_recv() {
                            samples = samples_;
                            s = s_;
                            speed_factor = speed_factor_;
                            playing.set(true);
                        }
                    }
                }
            }
            wait_on_pause!();
            while let Some((l_elem, r_elem)) = iter.next() {
                //printinfo!(self, "Playing len: ({},{}), s: {}", samples[0].len(), samples[1].len(), s);
                *l_elem = samples[0][s as usize].to_f32();
                *r_elem = samples[1][s as usize].to_f32();
                s += speed_factor;
                if s < 0.0 || s as usize >= samples[0].len() {
                    tr.send(s).unwrap();
                    let (samples_, s_, speed_factor_) = r.recv().unwrap();
                    samples = samples_;
                    s = s_;
                    speed_factor = speed_factor_;
                    wait_on_pause!();
                }
            }
            tr.send(s).unwrap();

            JackControl::Continue
        });
        let active_client = AsyncClient::new(client, (), process).unwrap();
        for (sink,src) in active_client.ports(None, Some(AudioOutSpec::default().jack_port_type()), PortFlags::from_bits(0x4|0x1).unwrap()).iter().zip(port_names.iter()) {
            printinfo!(self, "Connect {} -> {}", src, sink);
            active_client.connect_ports_by_name(src, sink).unwrap();
        }

        self.feed(active_client.sample_rate() as u32, rx, tx, t, rr);
        active_client.deactivate().unwrap();
    }
}
