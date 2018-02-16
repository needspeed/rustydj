//MAIN
function connect_midi_to_ws(socket) {
    var log = console.log.bind(console);
    var midi;

    function onMIDISuccess(midiAccess){
        midi = midiAccess;
        var inputs = midi.inputs.values();

        socket.send(JSON.stringify(
            {
                SetupMIDI: "DN-SC2000"
            }
        ));
        // loop through all inputs
        for (var input = inputs.next(); input && !input.done; input = inputs.next()) {
            // listen for midi messages
            input.value.onmidimessage = onMIDIMessage;
            listInputs(input);
        }
        // listen for connect/disconnect message
        midi.onstatechange = onStateChange;
    }

    function onMIDIMessage(event){
        var data = event.data; 
        //log('data', data);
        bytes = [0,0,0];
        for (i = 0; i < data.length; i++) {
            bytes[i] = data[i];
        }

        socket.send(JSON.stringify(
            {
                MIDI: ["DN-SC2000", bytes]
            }
        ));
    }

    function onStateChange(event){
        var port = event.port, state = port.state, name = port.name, type = port.type;
        if(type == "input")
            log("name", name, "port", port, "state", state);

    }

    function listInputs(inputs){
        var input = inputs.value;
        log("Input port : [ type:'" + input.type + "' id: '" + input.id + 
            "' manufacturer: '" + input.manufacturer + "' name: '" + input.name + 
            "' version: '" + input.version + "']");
    }

    //MAIN
    if (navigator.requestMIDIAccess) {
        navigator.requestMIDIAccess({sysex: false}).then(onMIDISuccess, function(e) {
            log("No access to MIDI devices or your browser doesn't support WebMIDI API. Please use WebMIDIAPIShim " + e);
        });
    }
    else {
        alert("No MIDI support in your browser.");
    }

}
