#![feature(concat_idents)]
extern crate simplemad;
extern crate cpal;
extern crate crossbeam;
extern crate minidom;
extern crate jack;
extern crate quick_xml;
extern crate url;
extern crate termion;
extern crate tui;
extern crate ws;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

#[allow(unreachable_patterns)]
mod ui;
#[allow(unreachable_patterns)]
mod mp3playerjack;
#[allow(unreachable_patterns)]
#[allow(unused)]
mod library;
#[allow(unused)]
mod controller;
#[allow(unreachable_patterns)]
mod textui;
#[allow(unreachable_patterns)]
mod wsui;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use mp3playerjack::{PlayerCommand, PlayerStatus};
use ui::UIType;

fn main() {
    let (tx, rx) = mpsc::channel::<PlayerCommand>();
    let (tx_r, rx_r) = mpsc::sync_channel::<PlayerStatus>(20);
    let mut player = mp3playerjack::Mp3Player::new();
    thread::spawn(move || {
            player.play(rx, tx_r);
    });
    //player.run(rx, tx_r);

    let mut args = std::env::args();
    println!("Args: {:?}", std::env::args().collect::<Vec<String>>());
    args.next().unwrap();
    let uitype : UIType = match args.next() {
        Some(s) => match s.as_ref() {
            "ws" => UIType::WS,
            "tui" => UIType::Tui,
            "simple" => UIType::Simple,
            _ => {
                println!("Unknown UI type: {}", s);
                return;
            }
        },
        _ => UIType::Simple,
    };

    ui::run(uitype, tx, rx_r, Arc::new(Mutex::new(library::Library::from_rb("rb_out.xml"))));
}
