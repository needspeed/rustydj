use std::sync::mpsc;
use std::cell::Cell;

use library::Library;
use mp3playerjack::PlayerCommand;
use ui::UICommand;

const PRINT: bool = true;


pub enum Controller {
    DNSC2000 {shift: bool, pitch_range_div: u32},
    LPK25,
}


impl Controller {
    pub fn new(aliases : Vec<String>) -> Option<Self> {
        if aliases.iter().any(|a| {a.contains("DN-SC2000")}) {
            return Some(Controller::DNSC2000{shift: false, pitch_range_div: 1<<4});
        }
        if aliases.iter().any(|a| {a.contains("LPK25")}) {
            return Some(Controller::LPK25);
        }
        None
    }
    
    pub fn handle_midi(&mut self, bytes: &[u8], tx : &mpsc::Sender<PlayerCommand>, txui: &mpsc::Sender<UICommand>) {
        macro_rules! printinfo {
            ($($args:expr),*) => {{
                if PRINT {
                    println!($($args),*);
                }
                else {
                   txui.send(UICommand::Print(format!($($args),*))).is_ok();
                }
            }};
        }

        fn print_midi(bytes : &[u8]) -> String {
            let status = bytes[0];
            let opcode = status >> 4;
            let channel = status & 0x0f;
            
            let mut outstr = String::new();
            outstr.push_str("MIDI [");
            for byte in bytes.iter() {
                outstr.push_str(&format!("{:02X}", byte));
            }
            outstr.push_str("] ");
            outstr.push_str(&format!("OP: {:X}, CN: {:X}, Rest: ", opcode, channel)); 
            for byte in bytes[1 .. ].iter() {
                outstr.push_str(&format!("{:02X}", byte));
            }
            outstr.push_str("| ");
            outstr
        }

        let status = bytes[0];
        let opcode = status >> 4;
        //let channel = status & 0x0f;
        match self {
            &mut Controller::LPK25 => {
                match opcode {
                    0x9 | 0x8 => {
                        let note = bytes[1];
                        let on = opcode == 0x9;
                        let mut has_matched = true;
                        match note {
                            0x30 => tx.send(PlayerCommand::HotCue(0, on)).unwrap(),
                            0x32 => tx.send(PlayerCommand::HotCue(1, on)).unwrap(),
                            0x34 => tx.send(PlayerCommand::HotCue(2, on)).unwrap(),
                            0x35 => tx.send(PlayerCommand::HotCue(3, on)).unwrap(),
                            0x37 => tx.send(PlayerCommand::HotCue(4, on)).unwrap(),
                            0x39 => tx.send(PlayerCommand::HotCue(5, on)).unwrap(),
                            0x3B => tx.send(PlayerCommand::HotCue(6, on)).unwrap(),
                            0x3D => tx.send(PlayerCommand::HotCue(7, on)).unwrap(),
                            0x3E => txui.send(UICommand::Enter).unwrap(),
                            0x3F => tx.send(PlayerCommand::PlayPause).unwrap(),
                            _ => has_matched = false,
                        };
                        if on && !has_matched {
                            match note {
                                _ => printinfo!("MIDI CC: {:X}| ", note), 
                            };
                        }
                    },
                    0x8 => {
                    },
                    _ => printinfo!("{}", print_midi(bytes)),
                }
            },
            &mut Controller::DNSC2000{ref mut shift, ref mut pitch_range_div} => {
                const SKIP_SPEED : f64 = 4.0;
                const PITCH_RANGES : u32 = 6;

                match opcode {
                    0x9 | 0x8 => {
                        let note = bytes[1];
                        let on = opcode == 0x9;
                        let mut has_matched = true;
                        match note {
                            0xC => tx.send(PlayerCommand::SpeedDiff(if on {SKIP_SPEED} else {1.0})).unwrap(),
                            0xD => tx.send(PlayerCommand::SpeedDiff(if on {-SKIP_SPEED} else {1.0})).unwrap(),
                            0x17 => tx.send(PlayerCommand::HotCue(0, on)).unwrap(),
                            0x18 => tx.send(PlayerCommand::HotCue(1, on)).unwrap(),
                            0x19 => tx.send(PlayerCommand::HotCue(2, on)).unwrap(),
                            0x20 => tx.send(PlayerCommand::HotCue(3, on)).unwrap(),
                            0x21 => tx.send(PlayerCommand::HotCue(4, on)).unwrap(),
                            0x22 => tx.send(PlayerCommand::HotCue(5, on)).unwrap(),
                            0x23 => tx.send(PlayerCommand::HotCue(6, on)).unwrap(),
                            0x24 => tx.send(PlayerCommand::HotCue(7, on)).unwrap(),
                            0x42 => tx.send(PlayerCommand::Cue(on)).unwrap(),
                            0x60 => *shift = on,
                            _ => has_matched = false,
                        };
                        if on && !has_matched {
                            match note {
                                0x10 => tx.send(PlayerCommand::CueMove(true)).unwrap(),
                                0x11 => tx.send(PlayerCommand::CueMove(false)).unwrap(),
                                0x43 => tx.send(PlayerCommand::PlayPause).unwrap(),
                                0x28 => txui.send(UICommand::Enter).unwrap(),
                                0x30 => txui.send(UICommand::Back).unwrap(),
                                0x6B => {
                                    *pitch_range_div <<= 1;
                                    if (*pitch_range_div & (1 << PITCH_RANGES)) > 0 {
                                        *pitch_range_div = 1;
                                    }
                                    txui.send(UICommand::PitchRange(
                                            1.0 - (0.5/ *pitch_range_div as f64), 
                                            1.0 + (1.0/ *pitch_range_div as f64)
                                            )).unwrap();
                                },
                                _ => printinfo!("MIDI CC: {:X}| ", note), 
                            };
                        }
                    }, 
                    0xB => {
                        let control = bytes[1];
                        let value = bytes[2];
                        match control {
                            0x51 => {
                                //0x0->0x40->0x7F
                                let value_fixed = if (value < 0x40) { value + 1 } else { value - 1 }; // no 0x40 on this wheel
                                let speed_factor = if value < 0x40 {
                                    (value + 0x40) as f64 / 0x80 as f64  //0.5 -> 1
                                }
                                else {
                                    (value - 0x40) as f64 * (0x40 as f64 / (0x7F-0x40) as f64) / 0x40 as f64 + 1.0
                                };

                                let normalized = (value_fixed as i32 - 0x40) as f64 / 64.0; // -1 -> 1
                                tx.send(PlayerCommand::Scratch(normalized)).unwrap();
                                //tx.send(PlayerCommand::SpeedDiff(speed_factor)).unwrap();
                            },
                            0x54 => txui.send(UICommand::Scroll(if value&0x1==1 {-1} else {1})).unwrap(),
                            _ => {
                                printinfo!("MIDI RR: {:02X} = {:02X}| ", control, value);                            
                            },
                        }
                    },
                    0xD | 0xC => {
                        //if bytes.len() == 2 {
                        //    let encoded : u8 = bytes[1]; //0x00->0x40->0x80 
                        //}
                    },
                    0xE => {
                        if bytes.len() == 3 {
                            let encoded : u16 = (bytes[2] as u16) << 8 | (bytes[1] as u16);//0x0000->0x4000->0x7F7F 
                            let speed_factor = if encoded < 0x4000 {
                                1.0 - (1.0 - ((encoded + 0x4000) as f64 / 0x8000 as f64)) / *pitch_range_div as f64  //0.5 -> 1
                            }
                            else {
                                1.0 + ((encoded - 0x4000) as f64 * (0x4000 as f64 / (0x7F7F-0x4000) as f64) / 0x4000 as f64) 
                                    / *pitch_range_div as f64
                            };
                            //println!("Speed_factor {}", speed_factor);
                            //println!("Speed_factor {:X}", encoded);
                            tx.send(PlayerCommand::Speed(speed_factor)).unwrap();
                        }
                    },
                    0xF => (),
                    _ => {
                        print_midi(bytes);
                    }
                };
            },
        };
    }    
}
