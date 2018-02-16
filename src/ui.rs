extern crate serde_json;

use std::thread;
use std::time::Duration;
use std;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;

use library::{Library, Track, LibraryCommand, LibraryResponse};
use mp3playerjack::{PlayerCommand, PlayerStatus};

use wsui;
use textui;

pub enum UIType {
    Simple,
    Tui,
    WS,
}

#[derive(Debug, Serialize)]
pub enum UICommand {
    Enter,
    Back,
    Scroll(i32),
    Quit,
    PitchRange(f64, f64),
    Print(String),
    ForwardStatus(PlayerStatus),
    ForwardLibrary(LibraryResponse)
}

#[derive(Debug, Deserialize)]
pub enum UIBackCommand {
    ForwardLibraryCommand(LibraryCommand),
    ForwardPlayerCommand(PlayerCommand),
    SetupMIDI(String),
    MIDI(String, [u8;3]),
}

impl UICommand {
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

impl UIBackCommand {
    pub fn deserialize(data: &str) -> Result<UIBackCommand, &'static str> {
        match serde_json::from_str(data) {
            Ok(x) => Ok(x),
            Err(_) => Err("Could not parse"),
        }
    }
}

pub fn run(ui_type: UIType, tx : mpsc::Sender<PlayerCommand>, rx_r : mpsc::Receiver<PlayerStatus>, library: Arc<Mutex<Library>>) {
    let (txui, rxui) = mpsc::channel::<UICommand>();
    midi(tx.clone(), txui.clone());
    match ui_type {
        UIType::Simple => text(tx, rx_r, rxui, library),
        UIType::Tui => textui::run(tx, rx_r, rxui, txui, library),
        UIType::WS => wsui::run(tx, rx_r, rxui, txui, library),
    }
}

fn text(mut tx : mpsc::Sender<PlayerCommand>, rx_r : mpsc::Receiver<PlayerStatus>, rxui: mpsc::Receiver<UICommand>, library: Arc<Mutex<Library>>) {
    // RENDER
    thread::spawn(move || {
        let mut pos = Duration::from_secs(0);
        let mut sample_pos = 0.0;
        let mut duration = Duration::from_secs(0);
        let mut speed = 1.0;
        let mut track : Option<Track> = None;
        loop {
            if let Ok(cmd) = rx_r.recv() {
                match cmd {
                    PlayerStatus::Pos(pos_, sample_pos_) => { pos = pos_; sample_pos = sample_pos_; },
                    PlayerStatus::TrackInfo(track_, duration_, _sample_rate_) => { track = track_; duration = duration_; }
                    PlayerStatus::Speed(speed_) => speed = speed_,
                    _ => (),
                }
                if let Some(ref track_) = track {
                    //print!("\n\x1B[30G[UI]: [{:02}:{:02}:{:03}/{:02}:{:02}:{:03} |{:013.2}| ({:.3}x)]   \n",  
                    print!(
                        "\r\x1B[30G[UI]: [{:02}:{:02}:{:03}/{:02}:{:02}:{:03} |{:013.2}| ({:.3}x = {:.2} bpm)]                      \r",  
                           pos.as_secs()/60, pos.as_secs()%60, pos.subsec_nanos()/1000000,
                           duration.as_secs()/60, duration.as_secs()%60, duration.subsec_nanos()/1000000,
                           sample_pos, speed, speed * track_.bpm as f64
                    );
                    std::io::stdout().flush().is_ok();
                }
            }
        }
    });
    let tx_ = tx.clone();
    //KEYBOARD
    thread::spawn(move || {
        loop {
            let mut cmdstr = String::new();
            std::io::stdin().read_line(&mut cmdstr).ok();
            let mut cmd_split = cmdstr.split_whitespace();
            let cmd = cmd_split.next().unwrap();
            println!("Got: {}", cmd);
            match cmd.as_ref() {
                "GetPos" => { tx.send(PlayerCommand::GetPos).unwrap(); () },
                "Seek" => { tx.send(PlayerCommand::Seek(cmd_split.next().unwrap().parse().unwrap())).unwrap(); () },
                "SeekS" => { tx.send(PlayerCommand::SeekS(Duration::from_secs(cmd_split.next().unwrap().parse().unwrap()))).unwrap(); () },
                //"Open" => tx.send(PlayerCommand::Open(cmd_split.collect::<Vec<&str>>().join(" "))).unwrap(),
                _ => println!("Unknown command"),
            }
        }
    });
    tx = tx_;
    //EVENTS
    let mut index = 0;
    loop {
        if let Ok(cmd) = rxui.recv() {
            match cmd {
                UICommand::Enter => tx.send(PlayerCommand::Open(library.lock().unwrap().get(index))).unwrap(),
                UICommand::Scroll(value) => {
                    let mut library_ = library.lock().unwrap();
                    index = (library_.tracks.len() as i32 + index as i32 + value) as usize % library_.tracks.len();
                    let track = library_.get(index);
                    println!("Track: {} - {}", track.info["Artist"], track.info["Name"]);
                },
                UICommand::Quit => break,
                _ => (),
            }
        };
    }
}

use jack::prelude::{AsyncClient, MidiInPort, MidiInSpec, Client, ClosureProcessHandler,
                    JackControl, ProcessScope, client_options, PortFlags, PortSpec};
use controller::Controller;
fn midi(tx : mpsc::Sender<PlayerCommand>, txui: mpsc::Sender<UICommand>) {
    let client = Client::new("rustydj_midi", client_options::NO_START_SERVER).unwrap().0;
    let port_names = client.ports(None, Some(MidiInSpec::default().jack_port_type()), PortFlags::from_bits(0x4|0x2).unwrap());
    let ports = port_names.iter().filter_map(|name| {client.port_by_name(name)});
    println!("Midi sources:"); 
    let controllers = Box::new(
            ports.filter_map(|port| {
                let name = port.name().to_string();
                let names = port.aliases();
                println!("{} ({:?})", name, names);
                match Controller::new(names) {
                    Some(ctrl) => Some((name, ctrl)),
                    None => None,
                }
            }).collect::<Vec<(String, Controller)>>()
            );


    if controllers.len() == 0 {
        println!("Warning no midi device");
        return;
    }

    let controllers = Box::into_raw(controllers);
    for &mut(ref name, ref mut ctrl) in unsafe {(*controllers).iter_mut()} { 
        let tx_ = tx.clone();
        let txui_ = txui.clone();

        let client = Client::new(&format!("rustydj_midi_{}",name), client_options::NO_START_SERVER).unwrap().0;
        let mut port = client.register_port("in", MidiInSpec::default()).unwrap();
        let port_name = port.name().to_string();

        let process = ClosureProcessHandler::new(move |_client: &Client, ps: &ProcessScope| -> JackControl {
            let buffer = MidiInPort::new(&mut port, ps);

            for elem in buffer.iter() {
                ctrl.handle_midi(elem.bytes, &tx_, &txui_);
            }

            JackControl::Continue
        });
        let active_client = Box::new(AsyncClient::new(client, (), process).unwrap());
        println!("Connect {} -> {}", name, port_name);
        active_client.connect_ports_by_name(&name, &port_name).unwrap();
        Box::into_raw(active_client);
    }
}
