window.trackLength=0;
window.speed=1.0;
window.bpm=128.0;
var waitingToPlayID=0;

var exampleSocket = new WebSocket("ws://127.0.0.1:2794");

var playlist_tree = [];
var track_cache = {};
var playlist_name_cache = {};

function cur_playlist_obj() {
    if (playlist_tree.length > 0) {
        return playlist_tree[playlist_tree.length - 1];
    }
    else return null;
}

function shift_item(vector) {
    var p = cur_playlist_obj();
    p.i = Math.max(Math.min(p.i + vector, p.items.length-1),0);
    select_item(p.i);
}

function select_item(index) {
    var p = cur_playlist_obj();
    p.i = index;
    for (var i=0; i<p.items.length; i++) {
        if (i != index) {
            p.items[i].id = "";
        }
    }
    p.items[index].id = "selectedElement";
}

function send(obj) {
    exampleSocket.send(JSON.stringify(obj));
}

exampleSocket.onmessage = function (event) {
    var uicmd = JSON.parse(event.data);

    var p = cur_playlist_obj();
    if ("Enter" === uicmd) {
        var id = parseInt(p.items[p.i].data.id);
        waitingToPlayID=id;
        if (p.is_node) {
            send({
                ForwardLibraryCommand: {
                    GetPlaylist: id
                }
            });
        }
        else {
            send({
                ForwardLibraryCommand: {
                    GetTrack: id
                }
            });
        }
    }
    else if ("Back" === uicmd) {
        id = p.playlist.parent;
        if (id != null) {
            var table = document.getElementById('playlistTable');
            table.innerHTML="";
            playlist_tree.pop();
            p = cur_playlist_obj();
            for(let i of p.items) {
                table.appendChild(i); 
            }
        }
    }
    else if (typeof uicmd === "object") {
        if ("Scroll" in uicmd) {
            var vector = uicmd.Scroll;
            shift_item(vector); 
        }
        else if ("ForwardLibrary" in uicmd) {
            var libcmd = uicmd.ForwardLibrary;
            if ("Playlist" in libcmd) {
                var playlist = libcmd.Playlist;

                if (playlist.id == waitingToPlayID && waitingToPlayID!=null) {
                    var playlist_items = [];
                    var table = document.getElementById('playlistTable');
                    table.innerHTML="";

                    var iter = playlist.track_keys;
                    var is_node = false;
                    if (playlist.sub_playlists.length > 0) {
                        is_node = true;
                        iter = playlist.sub_playlists;
                    }
                    for (let i of iter) {
                        var item = document.createElement('tr');
                        var s1 = document.createElement('td');
                        s1.id = "tableAlbumArt";
                        var s2 = document.createElement('td');
                        s2.id = "tableTitle";
                        var s3 = document.createElement('td');
                        s3.id = "tableCol3";

                        if (is_node) {
                            if (i in playlist_name_cache) {
                                p_name=playlist_name_cache[i];
                                s2.innerHTML=p_name;
                            }
                            else {
                                send({
                                    ForwardLibraryCommand: {
                                        GetPlaylist: i
                                    }
                                });
                                s2.innerHTML=i;
                            }
                        }
                        else {
                            if (i in track_cache) {
                                track=track_cache[i];
                                s2.innerHTML=track.Name;
                                //...
                            }
                            else {
                                send({
                                    ForwardLibraryCommand: {
                                        GetTrack: i
                                    }
                                });
                                s2.innerHTML=i;
                            }
                        }
                        item.data={id: i};
                        item.appendChild(s1);
                        item.appendChild(s2);
                        item.appendChild(s3);
                        table.appendChild(item);
                        playlist_items.push(item);
                    }
                    playlist_tree.push({ i: 0, playlist: playlist, items: playlist_items, is_node:is_node });
                    select_item(0);
                }
                else {
                    playlist_name_cache[playlist.id]=playlist.name;
                    var p = cur_playlist_obj();
                    for (let item of p.items) {
                        if (item.data.id==playlist.id) {
                            item.querySelector("#tableTitle").innerHTML=playlist.name;
                        }
                    }
                }
            }
            else if ("Track" in libcmd) {
                var track = libcmd.Track;
                if (track.id == waitingToPlayID && waitingToPlayID!=null) {
                    send({
                        ForwardPlayerCommand: {
                            Open: track
                        }
                    });
                    waitingToPlayID=null;
                }
                else {
                    track_cache[track.id]=track.info;
                    var p = cur_playlist_obj();
                    for (let item of p.items) {
                        if (item.data.id==track.id) {
                            item.querySelector("#tableTitle").innerHTML=track.info.Name;
                        }
                    }
                }
            }
            else console.log(uicmd);
        }
        else if ("ForwardStatus" in uicmd) {
            var statuscmd = uicmd.ForwardStatus;
            if ("TrackInfo" in statuscmd) {
                var trackinfo = statuscmd.TrackInfo;
                var track = trackinfo[0];
                var duration = trackinfo[1];
                var sample_rate = trackinfo[2];
                window.trackLength=duration.secs*1000+Math.floor(parseInt(duration.nanos)/1000000);
                //document.getElementById("Artist").innerHTML = track.info.Artist;
                document.getElementById("leftHeader").innerHTML = track.info.Name;
                //document.getElementById("Album").innerHTML = track.info.Album;
                //document.getElementById("BPM").innerHTML = track.bpm;
                window.bpm = track.bpm;
                speedChange(window.speed);
                document.getElementById("rightHeader").innerHTML = track.info.Tonality;
            }
            else if ("Pos" in statuscmd) {
                var pos = statuscmd.Pos;
                var time = pos[0];
                updateTime(time.secs*1000+Math.floor(parseInt(time.nanos)/1000000));
            }
            else if ("Speed" in statuscmd) {
                speedChange(statuscmd.Speed);
            }
            else console.log(uicmd);
        }
    }
}

function initializeUI() {
    clearTime();
    document.getElementById("playhead").style.left="0%";
    document.getElementById("progress").style.width="0%";
}

function clearTime() {
    document.getElementById("minutes").innerHTML="00";
    document.getElementById("seconds").innerHTML="00";
    document.getElementById("farts").innerHTML="00";
    document.getElementById("centiFarts").innerHTML="0";
}

function finalizeUI() {
    clearTime();
    document.getElementById("playhead").style.left="100%";
    document.getElementById("progress").style.width="100%";
}

function updateTime(curMilli) {
    timeLeft = window.trackLength-curMilli;
    var minutes = Math.floor(timeLeft / 60000);
    var seconds = ((timeLeft % 60000) / 1000).toFixed(0);
    var farts = (((timeLeft % 60000) % 1000) / 10).toFixed(0);
    var centiFarts = ((((timeLeft % 60000) % 1000) % 10)).toFixed(0);
    farts=farts%100;
    centiFarts=centiFarts%10;
    if(minutes<10){
        minutes = "0"+minutes;
    }
    if(seconds<10){
        seconds = "0"+seconds;
    }
    if(farts<10){
        farts = "0"+farts;
    }
    document.getElementById("minutes").innerHTML=minutes;
    document.getElementById("seconds").innerHTML=seconds;
    document.getElementById("farts").innerHTML=farts;
    document.getElementById("centiFarts").innerHTML=centiFarts;
    var playedPercent = ((window.trackLength-timeLeft)/(window.trackLength)*100).toFixed(2);
    document.getElementById("playhead").style.left=playedPercent+"%";
    document.getElementById("progress").style.width=playedPercent+"%";
}

function speedChange(newSpeed) {
    window.speed=newSpeed;
    var speedValue=((newSpeed*100)-100).toFixed(2);
    if(speedValue<0){
        speedValue=Math.abs(speedValue);
        document.getElementById("tempoPlus").innerHTML="-";
    }else{
        document.getElementById("tempoPlus").innerHTML="+";
    }
    document.getElementById("tempo").innerHTML=speedValue;
    document.getElementById("bpmNum").innerHTML=(window.bpm*newSpeed).toFixed(1);
}

window.onload = function() {
    initializeUI();
    exampleSocket.onopen = function (event) {
        connect_midi_to_ws(exampleSocket);
        exampleSocket.send(JSON.stringify({
            ForwardLibraryCommand: {
                GetPlaylist: 0
            }
        }));
    };
};
