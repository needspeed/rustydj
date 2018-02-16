extern crate minidom;
extern crate quick_xml;
extern crate url;

use std::fs::File;
use std::io::BufReader;
use minidom::Element;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use url::{Url, ParseError};
use std::sync::mpsc;

use ui::UICommand;

#[derive(Debug, Deserialize)]
pub enum LibraryCommand {
    GetPlaylist(usize),
    GetTrack(usize),
}

#[derive(Debug, Serialize)]
pub enum LibraryResponse {
    Track(Track),
    Playlist(Playlist),
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct Cue {
    pub name: String,
    pub start: usize,
    pub type_: u8,
}

#[derive(Debug, Serialize, Deserialize)]
#[derive(Clone)]
pub struct Track {
    pub id: usize,
    pub path : String,
    pub info : HashMap<String, String>,
    pub bpm : u32,
    pub sample_rate: u32,
    pub cues : Vec<Cue>,
    pub first_beat : usize,
}

#[derive(Clone)]
#[derive(Debug, Serialize)]
pub struct Playlist {
    pub name : String,
    pub sub_playlists : Vec<usize>,
    pub track_keys : Vec<usize>,
    pub parent: Option<usize>,
    pub id: usize,
}

//#[derive(Clone)]
//#[derive(Debug, Serialize)]
//pub struct MetaPlaylist {
//    pub playlist: Playlist,
//    pub track_data: Vec<Track>,
//}


pub struct Library {
    pub tracks : HashMap<usize, Track>,
    pub root_playlist : Option<usize>,
    pub playlists : Vec<Playlist>
}

impl Track {
    pub fn get_headers() -> Vec<&'static str> {
        return vec!["Artist", "Title", "Album", "Bpm", "Key"];
    }

    pub fn get(&self, typ: &str) -> String {
        match typ {
            "Artist" => self.artist(),
            "Title" => self.title(),
            "Album" => self.album(),
            "Bpm" => self.bpm().to_string(),
            "Key" => self.key(),
            "Id" => self.id().to_string(),
            _ => "?".to_string(),
        }
    }

    pub fn id(&self) -> usize {
        self.id
    }
    pub fn artist(&self) -> String {
        self.info["Artist"].clone()
    }
    pub fn title(&self) -> String {
        self.info["Name"].clone()
    }
    pub fn album(&self) -> String {
        self.info["Album"].clone()
    }
    pub fn bpm(&self) -> f64 {
        self.bpm as f64
    }
    pub fn key(&self) -> String {
        self.info["Tonality"].clone()
    }
}

impl Library {
    pub fn from_rb(path : &str) -> Library {
        println!("Parse rekordbox xml...");
        let f = File::open(path).expect("File not found");
        let f = BufReader::new(f);
        let mut reader = Reader::from_reader(f);
        let root = Element::from_reader(&mut reader).unwrap();

        assert_eq!(root.attr("Version").unwrap(),"1.0.0");
        let mut tracks = HashMap::<usize, Track>::new();
        let mut playlists = Vec::new();
        let mut root_playlist = None;

        for master_node in root.children() {
            match master_node.name() {
                "PRODUCT" => println!("Product: {:?}", master_node.attrs().collect::<Vec<_>>()),
                "COLLECTION" => {
                    'tracks: for track_node in master_node.children() {
                        if track_node.name() != "TRACK" {
                            println!("Warning");
                            continue 'tracks;
                        }
                        let info : HashMap<String, String> = track_node.attrs().map(|(k,v):(&str,&str)| {(k.to_string(), v.to_string())}).collect();
                        let sample_rate : u32 = info["SampleRate"].parse().unwrap();
                        let mut bpm : u32 = 0;
                        let mut cues : Vec<Cue> = Vec::new();
                        let mut first_beat : usize = 0;
                        let path = Url::parse(&info["Location"]).unwrap().to_file_path().unwrap().to_str().unwrap().to_string();
                        for track_sub_node in track_node.children() {
                            match track_sub_node.name() {
                                "TEMPO" => {
                                    bpm = track_sub_node.attr("Bpm").unwrap().parse::<f32>().unwrap() as u32;
                                    first_beat = (track_sub_node.attr("Inizio").unwrap().parse::<f32>().unwrap() 
                                                  * sample_rate as f32) as usize;
                                },
                                "POSITION_MARK" => cues.push(Cue {
                                    name: track_sub_node.attr("Name").unwrap().to_string(),
                                    start: (track_sub_node.attr("Start").unwrap().parse::<f32>().unwrap() * sample_rate as f32 - 1152.0*2.0) as usize,
                                    type_: track_sub_node.attr("Type").unwrap().parse().unwrap(),
                                }),
                                _ => println!("Warning"),
                            }
                        }
                        let id = info["TrackID"].parse::<usize>().unwrap();
                        tracks.insert(id, Track {
                            id: id,
                            path: path,
                            info: info,
                            bpm: bpm,
                            sample_rate: sample_rate,
                            cues: cues,
                            first_beat: first_beat,
                        });
                    }
                },
                "PLAYLISTS" => {
                    let mut ctr = 0;
                    fn parse_playlist(node: &minidom::Element, parent: Option<usize>, ctr : &mut usize) 
                        -> Result<(usize, Vec<Playlist>), ()> {
                        let mut out_list = Vec::new();
                        let id = ctr.clone();
                        match node.attr("Type").unwrap() {
                            "0" => {
                                let mut out = Playlist {
                                    name: node.attr("Name").unwrap().to_string(),
                                    sub_playlists: Vec::new(),
                                    track_keys: Vec::new(),
                                    parent: parent,
                                    id: id,
                                };
                                *ctr += 1;
                                for i in node.children() {
                                    let mut res = parse_playlist(i, Some(id), ctr).unwrap();
                                    out.sub_playlists.push(res.0);
                                    out_list.append(&mut res.1);
                                }
                                out_list.insert(0, out);
                                Ok((id, out_list))
                            },
                            "1" => {
                                out_list.push(Playlist {
                                    name: node.attr("Name").unwrap().to_string(),
                                    sub_playlists: Vec::new(),
                                    track_keys: node.children().map(|tn| tn.attr("Key").unwrap().parse::<usize>().unwrap()).collect(),
                                    parent: parent,
                                    id: id,

                                });
                                *ctr += 1;
                                Ok((id, out_list))
                            },
                            _ => Err(()),
                        }
                    }
                    let result = parse_playlist(master_node.children().next().unwrap(), None, &mut ctr).unwrap();
                    root_playlist = Some(result.0);
                    playlists = result.1;
                },
                _ => (),
            };
        }
        println!("Done parsing");

        Library {
            tracks: tracks,
            root_playlist: root_playlist,
            playlists: playlists,
        }
    }
    
    pub fn get(&self, id: usize) -> Track {
        self.tracks[&id].clone()
    }

    pub fn handle(&self, cmd: LibraryCommand, txui: &mpsc::Sender<UICommand>) {
        println!("[Library] Handle cmd");
        match cmd {
            LibraryCommand::GetTrack(id) => txui.send(UICommand::ForwardLibrary(LibraryResponse::Track(self.get(id)))).unwrap(),
            LibraryCommand::GetPlaylist(id) => txui.send(UICommand::ForwardLibrary(LibraryResponse::Playlist(self.playlists[id].clone()))).unwrap(),
            _ => println!("Unsupported librarycmd"),
        }
    }
}
