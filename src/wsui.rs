extern crate ws;

use ws::listen;
use ws::{Handler, Sender, Handshake, Result, Message, CloseCode};

use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use mp3playerjack::{PlayerCommand, PlayerStatus};
use ui::{UICommand,UIBackCommand};
use library::{Library};
use controller::Controller;
use std::collections::HashMap;

struct Server {
    out: Sender,
    rxui: Arc<Mutex<mpsc::Receiver<UICommand>>>,
    tx: mpsc::Sender<PlayerCommand>,
    txui: mpsc::Sender<UICommand>,
    library: Arc<Mutex<Library>>,
    controller: HashMap<String, Controller>,
}

impl Server {
    fn run(&self) {
        let s = self.out.clone();
        let rxui = self.rxui.clone();
        thread::spawn(move || {
            let rxui = rxui.lock().unwrap();
            loop {
                if let Ok(cmd) = rxui.recv() {
                    if let UICommand::Quit = cmd {
                        return;
                    }
                    s.send(cmd.serialize()).is_ok();
                }
            }
        });
    }
}

impl Handler for Server {
    fn on_open(&mut self, _: Handshake) -> Result<()> {
        self.run();
        Ok(())
    }

    fn on_close(&mut self, code: CloseCode, reason: &str) {
        match code {
            CloseCode::Normal => println!("The client is done with the connection."),
            CloseCode::Away   => println!("The client is leaving the site."),
            _ => println!("The client encountered an error: {}", reason),
        }
        self.txui.send(UICommand::Quit).unwrap();
    }

    fn on_message(&mut self, msg: Message) -> Result<()> {
        println!("{}", msg);
        if let Ok(uibc) = UIBackCommand::deserialize(&msg.to_string()) {
            println!("{:?}", uibc);
            match uibc {
                UIBackCommand::ForwardLibraryCommand(librarycmd) => self.library.lock().unwrap().handle(librarycmd, &self.txui),
                UIBackCommand::ForwardPlayerCommand(playercmd) => self.tx.send(playercmd).unwrap(),
                UIBackCommand::SetupMIDI(ctrl_name) => { 
                    let ctrl_name_ = ctrl_name.clone();
                    if let Some(ctrl) = Controller::new(vec![ctrl_name]) {
                        self.controller.insert(ctrl_name_.clone(), ctrl); 
                        println!("Added WS Midi controller: {}", ctrl_name_);
                    }},
                UIBackCommand::MIDI(ctrl_name, bytes) => { 
                    if let Some(ctrl) = self.controller.get_mut(&ctrl_name) { 
                        ctrl.handle_midi(&bytes, &self.tx, &self.txui);
                    }},
            }
        }
        Ok(())
    }
}

pub fn run(tx : mpsc::Sender<PlayerCommand>, rx_r : mpsc::Receiver<PlayerStatus>, rxui: mpsc::Receiver<UICommand>, txui: mpsc::Sender<UICommand>, library: Arc<Mutex<Library>>) {
    // MP3Player Status
    let txui_ = txui.clone();
    thread::spawn(move || {
        let mut last_speed = 0.0;
        let mut last_dur = Duration::default();
        let threshold = Duration::from_millis(10);
        let mut now = Instant::now();
        loop {
            if let Ok(cmd) = rx_r.recv() {
                match cmd {
                    PlayerStatus::Pos(dur, sampl) => {
                        if (dur > last_dur && dur - last_dur > threshold) || 
                            (dur < last_dur && last_dur - dur > threshold) {
                            last_dur = dur;
                            txui_.send(UICommand::ForwardStatus(cmd)).unwrap();
                        }
                    },
                    PlayerStatus::Speed(speed) => {
                        if last_speed != speed {
                            last_speed = speed;
                            if now.elapsed().subsec_nanos() > 100_000000 || now.elapsed().as_secs() > 0 {
                                txui_.send(UICommand::ForwardStatus(cmd)).unwrap();
                                now = Instant::now();
                            }
                        }
                    }, 
                    _ => txui_.send(UICommand::ForwardStatus(cmd)).unwrap(),
                }
            }
        }
    });

    let rxui_ = Arc::new(Mutex::new(rxui));
    
    listen("127.0.0.1:2794", |out| {
    //listen("0.0.0.0:2794", |out| {
        Server {out: out, rxui: rxui_.clone(), tx: tx.clone(), library: library.clone(), txui: txui.clone(), controller: HashMap::new()}
    }).unwrap();
}
