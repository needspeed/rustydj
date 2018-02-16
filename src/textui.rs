extern crate termion;
extern crate tui;

use std::io;

use termion::event;
use termion::input::TermRead;

use tui::Terminal;
use tui::backend::{RawBackend, Backend};
use tui::widgets::{Block, Borders, Row, Table, Widget, Paragraph, Gauge, Tabs};
use tui::layout::{Direction, Group, Rect, Size};
use tui::style::{Color, Modifier, Style};

use std::collections::HashMap;
use std::cmp::{min, max};
use std::time::{Duration, Instant};
use std::ops::{DerefMut, Div};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::fmt;
use mp3playerjack::{PlayerCommand, PlayerStatus};
use ui::UICommand;
use library::{Library, Track};

impl fmt::Display for Duration_ {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let secs = self.unwrap().as_secs();
        let nanos = self.unwrap().subsec_nanos();
        write!(f, "{:02}:{:02}:{:03}", secs/60, secs%60, nanos/1000000)
    }
}

struct Duration_ {
    inner: Duration
}

impl Duration_ {
    fn default() -> Duration_ {
        Duration_ {
            inner: Duration::default(),
        }
    }

    fn new(duration: Duration) -> Duration_ {
        Duration_ {
            inner: duration
        }
    }

    fn unwrap(&self) -> &Duration {
        &self.inner
    }
}

impl<'a,'b> Div<&'a Duration_> for &'b Duration_ {
    type Output = f64;

    fn div (self, rhs: &Duration_) -> f64 {
        self.unwrap().as_secs() as f64 / rhs.unwrap().as_secs() as f64
    }
}

struct LibraryRender<'a> {
    size: Rect,
    tracks: HashMap<usize, Vec<String>>,
    item_indexes: Vec<usize>,
    items: HashMap<usize, Vec<String>>,
    selected: usize,
    selected_id: usize,
    headers: Vec<&'a str>,
    top: usize,
    bottom: usize,
    playlists: HashMap<usize, (bool, HashMap<usize, Vec<String>>)>,
    playlist_stack: Vec<usize>,
    playlist_names: Vec<String>,
}

struct TrackRender {
    duration: Duration_,
    position: Duration_,
    sample_pos: f64,
    speed: f64,
    track: Option<Track>,
}

struct DebugRender {
    buffer: Vec<String>
}

struct App<'a> {
    size: Rect,
    terminal: Terminal<RawBackend>,
    debugr: DebugRender,
    trackr: TrackRender,
    libraryr: LibraryRender<'a>,
}

impl<'a> App<'a> {
    fn new(library: &Library) -> App<'a> {
        // Terminal initialization
        //let backend = MouseBackend::new().unwrap();
        let backend = RawBackend::new().unwrap();
        App { 
            size: Rect::default(),
            terminal: Terminal::new(backend).unwrap(),
            libraryr: LibraryRender::new(library),
            debugr: DebugRender::new(),
            trackr: TrackRender::new(),
        }
    }

    fn draw(&mut self) {
        let trackr = &mut self.trackr;
        let debugr = &mut self.debugr;
        let libraryr = &mut self.libraryr;
        Group::default()
            .direction(Direction::Vertical)
            .sizes(&[Size::Max(15), Size::Percent(50)])
            .margin(1)
            .render(&mut self.terminal, &self.size.clone(), |t, chunks| {
                trackr.render(t, &chunks[0]);
                Group::default()
                    .direction(Direction::Horizontal)
                    .sizes(&[Size::Max(chunks[1].width), Size::Percent(20)])
                    .margin(0)
                    .render(t, &chunks[1], |t, chunks| {
                        libraryr.render(t, &chunks[0]);
                        debugr.render(t, &chunks[1]);
                    });
            });
        //debugr.println(format!("Top: {}, Bottom: {}, Selected: {}, Selected_ID: {}", libraryr.top, libraryr.bottom, libraryr.selected, libraryr.selected_id));

        self.terminal.draw().unwrap();
    }

    fn resize(&mut self) {
        let size = self.terminal.size().unwrap();
        if size != self.size {
            self.terminal.resize(size).unwrap();
            self.size = size;
        }
    }

    //TODO
    fn destr(&mut self) {
        self.terminal.show_cursor().unwrap();
        self.terminal.clear().unwrap();
    }
}

impl TrackRender {
    fn new() -> TrackRender {
        TrackRender {
            duration: Duration_::default(),
            position: Duration_::default(),
            sample_pos: 0.0,
            speed: 1.0,
            track : None,
        }
    }

    fn render<T: Backend>(&mut self, t: &mut Terminal<T>, chunk: &Rect) {
        let mut to_print = String::new();
        let mut track_str = String::new();
        if let Some(ref track_) = self.track {
            to_print.push_str(&format!("Speed: {:.2} ({:.3}x)", self.speed * track_.bpm() as f64, self.speed));
            track_str.push_str(&format!("Artist: {}\nTitle: {}\nAlbum: {}\nKey: {}\nBPM: {}", 
                                        track_.artist(), track_.title(), track_.album(), track_.key(), track_.bpm()));
        }

        Block::default()
            .title("Now playing")
            .borders(Borders::ALL)
            .render(t, chunk);
        Group::default()
            .direction(Direction::Vertical)
            .sizes(&[Size::Max(10), Size::Max(5)])
            .margin(1)
            .render(t, chunk, |t, chunks| {
                Group::default()
                    .direction(Direction::Horizontal)
                    .sizes(&[Size::Percent(40), Size::Percent(30)])
                    .margin(0)
                    .render(t, &chunks[0], |t, chunks| {
                        Paragraph::default()
                            .block(Block::default()
                                   .title("Track")
                                   .borders(Borders::ALL))
                            .text(&track_str)
                            .render(t, &chunks[0]);
                        Paragraph::default()
                            .block(Block::default()
                                   .title("Info")
                                   .borders(Borders::ALL))
                            .text(&to_print)
                            .render(t, &chunks[1]);
                    });
                Gauge::default()
                    .block(Block::default())
                    .style(
                        Style::default()
                        .fg(Color::Magenta)
                        .bg(Color::Black)
                        .modifier(Modifier::Italic),
                        )
                    .label(&format!("{}/{} |{:013.2}|", self.position, self.duration, self.sample_pos))
                    .percent((&self.position/&self.duration * 100.0) as u16)
                    .render(t, &chunks[1]);
            });
    }
}

impl DebugRender {
    fn new() -> DebugRender {
        DebugRender {
            buffer: Vec::new(),
        }
    }

    fn println(&mut self, msg : String) {
        self.buffer.push(msg);
    }

    fn render<T: Backend>(&mut self, t: &mut Terminal<T>, chunk: &Rect) {
        let buf_low_index = max(self.buffer.len() as isize - chunk.height as isize, 0) as usize;
        Paragraph::default()
            .block(Block::default()
                   .title("Debug")
                   .borders(Borders::ALL))
            .wrap(false)
            .text(&self.buffer[buf_low_index .. ].join("\n"))
            .render(t, chunk);
    }
}

impl<'a> LibraryRender<'a> {
    fn new(library: &Library) -> LibraryRender<'a> {
        let tracks_ : &HashMap<usize, Track> = &library.tracks;
        let headers = vec!["Idx", "Id", "Title", "Artist", "Key", "Bpm"];
        let tracks : HashMap<usize, Vec<String>> = tracks_.iter().map(|(i, track) : (&usize, &Track) | (*i, headers[1..].iter().map(|header| track.get(header)).collect())).collect();

        let root_playlist = library.root_playlist.unwrap();
        let pl_names : Vec<String> = library.playlists.iter().map(|pl| pl.name.clone()).collect();
        let playlists : HashMap<usize, (bool, HashMap<usize, Vec<String>>)> = 
            library.playlists.iter().enumerate().map(|(i, pl)| {
                let is_track_list = pl.track_keys.len() > 0;
                (i, (is_track_list, match is_track_list {
                    false => pl.sub_playlists.iter().map(|is| (*is, vec![is.to_string(), pl_names[*is].clone()])).collect::<HashMap<usize, Vec<String>>>(),
                    true => pl.track_keys.iter().map(|i| (*i, tracks[i].clone())).collect::<HashMap<usize, Vec<String>>>(),
                }))}).collect();

        let mut out = LibraryRender {
            size: Rect::default(),
            tracks: tracks,
            items: HashMap::new(),
            item_indexes: vec![],
            headers: headers,
            selected: 0,
            top: 0,
            bottom: 0,
            selected_id: 0,
            playlists: playlists,
            playlist_stack: vec![root_playlist],
            playlist_names: pl_names,
        };
        out.set_playlist(root_playlist);
        return out;
    }

    fn back(&mut self) {
        if self.playlist_stack.len() > 1 {
            self.playlist_stack.pop();
            let playlist_id = self.playlist_stack.last().unwrap().clone();
            self.set_playlist(playlist_id);
        }
    }

    fn select(&mut self, tx: &mpsc::Sender<PlayerCommand>, library: &mut Library) {
        let cur_pl_id = self.playlist_stack.last().unwrap().clone();
        let cur_pl = self.playlists[&cur_pl_id].clone();
        let is_tracklist = cur_pl.0;
        match  is_tracklist {
            true => tx.send(PlayerCommand::Open((*library).get(self.selected_id))).unwrap(),
            false => {
                let mut item_indexes : Vec<usize> = cur_pl.1.keys().map(|x| *x).collect();
                item_indexes.sort();
                let selected = self.selected;
                let playlist_id = item_indexes[selected];
                self.playlist_stack.push(playlist_id);
                self.set_playlist(playlist_id);
            }
        }
    }

    fn set_playlist(&mut self, id: usize) {
        self.items = self.playlists[&id].1.clone();
        let sort_column = if self.items.values().next().unwrap().len()>=3 {3} else {1};
        let mut indexes : Vec<(&usize, &Vec<String>)> = self.items.iter().collect();
        indexes.sort_unstable_by(|&(_, val1), &(_, val2)| val1[sort_column].cmp(&val2[sort_column]));
        self.item_indexes = indexes.iter().map(|x| *x.0).collect();
        self.bottom = min(self.items.len(), self.size.height as usize);
        self.selected = 0;
        self.selected_id = self.item_indexes[self.selected];
    }

    fn shift_select(&mut self, vector: isize) {
        let siz = self.size.height as isize;
        let top = self.top as isize;
        let bot = self.bottom as isize;
        let mut selected = self.selected as isize;
        let items = self.items.len() as isize;
        //self.selected = ((items + selected + vector) % items) as usize;
        self.selected = min(max(selected + vector, 0), items-1) as usize;
        selected = self.selected as isize;
        if selected < top {
            self.top = selected as usize;
            self.bottom = min(siz + top, items) as usize;
        }
        if self.selected >= self.bottom {
            self.bottom = selected  as usize + 1;
            self.top = max(bot - siz, 0) as usize;
        }
        self.selected_id = self.item_indexes[self.selected];
    }

    fn update_size(&mut self, size: Rect) {
        let siz = size.height as isize;
        let top = self.top as isize;
        let bot = self.bottom as isize;
        let selected = self.selected as isize;
        let items = self.items.len() as isize;
        if siz  < bot - top {
            if selected - top < siz {
                self.bottom = min(siz + top, items) as usize;
            }
            else {
                self.bottom = selected as usize + 1;
                self.top = max(bot - siz, 0) as usize;
            }
        }
        else if siz > bot - top {
            self.bottom = min(siz - top, items - top) as usize;
        }
        self.size = size;
    }

    fn render<T: Backend>(&mut self, t: &mut Terminal<T>, chunk: &Rect) {
        Group::default()
            .direction(Direction::Vertical)
            .sizes(&[Size::Fixed(3), Size::Min(0)])
            .render(t, chunk, |t, chunks| {
                Tabs::default()
                    .block(Block::default().borders(Borders::ALL).title("Library"))
                    .titles(&self.playlist_stack.iter().map(|k| self.playlist_names[*k].clone()).collect::<Vec<String>>()[..])
                    .style(Style::default().fg(Color::Green))
                    .highlight_style(Style::default().fg(Color::Yellow))
                    .select(self.playlist_stack.len()-1)
                    .render(t, &chunks[0]);
                let cur_pl_id = self.playlist_stack.last().unwrap().clone();
                let cur_pl = self.playlists[&cur_pl_id].clone();
                let is_tracklist = cur_pl.0;

                let chunk = &chunks[1];
                self.update_size(Rect::new(0,0, chunk.width, chunk.height-4));
                let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::Bold);
                let normal_style = Style::default().fg(Color::White);
                let row_ids : Vec<Vec<String>> = (self.top..self.bottom).into_iter().map(|i| vec![i.to_string()]).collect();
                Table::new(
                    if is_tracklist {self.headers.iter()} else {["Idx", "Id", "Name"].iter()},
                    self.item_indexes[self.top .. self.bottom].iter().enumerate().map(|(i_, key)| {
                        let i = self.top + i_;
                        let item = &self.items[key];
                        let style = if i == self.selected { &selected_style } else { &normal_style };

                        let iter = row_ids[i_].iter().chain(item.into_iter());
                        Row::StyledData(iter, style)
                    })
                    ).block(Block::default().borders(Borders::ALL).title(if is_tracklist {"Tracks"} else {"Playlists"}))
                    .widths(&(if is_tracklist {[
                            4, 
                            4,
                            ((chunk.width-10-6-6) as f32 * 2.0/3.0) as u16, 
                            ((chunk.width-10-6-6) as f32 * 1.0/3.0) as u16, 
                            3, 
                            3]} else {[4,4,chunk.width-14,0,0,0]}))
                    .render(t, chunk);
            });
    }
}

fn handle_keyboard(txui: &mpsc::Sender<UICommand>, txplayer: &mpsc::Sender<PlayerCommand>, key: termion::event::Key) -> bool {
    match key {
        event::Key::Char('q') => {
            txui.send(UICommand::Quit).unwrap();
            return false;
        }
        event::Key::Down => txui.send(UICommand::Scroll(1)).unwrap(),
        event::Key::Up => txui.send(UICommand::Scroll(-1)).unwrap(),
        event::Key::Char('\n') => txui.send(UICommand::Enter).unwrap(),
        event::Key::Char(' ') => txplayer.send(PlayerCommand::PlayPause).unwrap(),
        event::Key::Backspace => txui.send(UICommand::Back).unwrap(),
        //event::Key::Char('c') => txplayer.send(PlayerCommand::Cue).unwrap(),
        //event::Key::Char('1') => txplayer.send(PlayerCommand::HotCue(0)).unwrap(),
        //event::Key::Char('2') => txplayer.send(PlayerCommand::HotCue(1)).unwrap(),
        //event::Key::Char('3') => txplayer.send(PlayerCommand::HotCue(2)).unwrap(),
        //event::Key::Char('4') => txplayer.send(PlayerCommand::HotCue(3)).unwrap(),
        //event::Key::Char('5') => txplayer.send(PlayerCommand::HotCue(4)).unwrap(),
        //event::Key::Char('6') => txplayer.send(PlayerCommand::HotCue(5)).unwrap(),
        //event::Key::Char('7') => txplayer.send(PlayerCommand::HotCue(6)).unwrap(),
        //event::Key::Char('8') => txplayer.send(PlayerCommand::HotCue(7)).unwrap(),
        _ => {}
    };
    true
}

fn handle_event(cmd : UICommand, app : &mut App, tx: &mpsc::Sender<PlayerCommand>, library: &mut Library) -> bool {
    match cmd {
        UICommand::Enter => app.libraryr.select(tx, library),
        UICommand::Back => app.libraryr.back(),
        UICommand::Scroll(value) => {
            app.libraryr.shift_select(value as isize);
        },
        UICommand::Quit => {
            app.destr();
            return false;
        },
        UICommand::ForwardStatus(playerstatus) => {
            match playerstatus {
                PlayerStatus::Pos(pos, sample_pos) => {
                    app.trackr.position = Duration_::new(pos);
                    app.trackr.sample_pos = sample_pos;
                }
                PlayerStatus::TrackInfo(track, duration, _sample_rate_) => { 
                    app.trackr.track = track;  
                    app.trackr.duration = Duration_::new(duration);
                }
                PlayerStatus::Speed(speed) => app.trackr.speed = speed,
                PlayerStatus::Print(msg) => app.debugr.println(msg),
                _ => (),
            }
        },
        UICommand::PitchRange(low, high) => app.debugr.println(format!("Pitch range: [{} -> {}]", low, high)), 
        UICommand::Print(msg) => app.debugr.println(msg),
    _ => (),
    };
    true
}

pub fn run(tx : mpsc::Sender<PlayerCommand>, rx_r : mpsc::Receiver<PlayerStatus>, rxui: mpsc::Receiver<UICommand>, txui: mpsc::Sender<UICommand>, library: Arc<Mutex<Library>>) {

    // App
    let app_ = Arc::new(Mutex::new(App::new(&library.lock().unwrap()))); 
    {
        let mut app = app_.lock().unwrap();
        // First draw call
        app.terminal.clear().unwrap();
        app.terminal.hide_cursor().unwrap();
        app.size = app.terminal.size().unwrap();
        app.draw();
    }

    let txui_ = txui.clone();
    let tx_ = tx.clone();
    // KEYBOARD
    thread::spawn(move || {
        let stdin = io::stdin();
        for c in stdin.keys() {
            if !handle_keyboard(&txui_, &tx_, c.unwrap()) {
                break;
            }
        }
    });


    // MP3Player Status
    thread::spawn(move || {
        loop {
            if let Ok(cmd) = rx_r.recv() {
                txui.send(UICommand::ForwardStatus(cmd)).unwrap();
            }
        }
    });
    let mut app = app_.lock().unwrap();

    // EVENTS
    let mut now = Instant::now();
    loop {

        if let Ok(cmd) = rxui.recv() {
            if !handle_event(cmd, &mut app, &tx, library.lock().unwrap().deref_mut()) {
                break;
            }
            if now.elapsed().subsec_nanos() > 20000000 || now.elapsed().as_secs() > 0 {
                app.resize();
                app.draw();
                now = Instant::now();
            }
        };
    }
}
