use anyhow::{Context, Result};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use parking_lot::Mutex;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::core::config::AppConfig;
use crate::input::mapping::KeyMappingEngine;
use crate::input::simulator::InputSimulator;
use crate::synth::engine::PianoSynthEngine;

pub struct MidiIoManager {
    input_conn: Arc<Mutex<Option<MidiInputConnection<()>>>>,
    output_conn: Arc<Mutex<Option<MidiOutputConnection>>>,
    synth: Arc<Mutex<PianoSynthEngine>>,
    simulator: Arc<InputSimulator>,
    mapping: Arc<Mutex<KeyMappingEngine>>,
    config: Arc<Mutex<AppConfig>>,
}

impl MidiIoManager {
    pub fn new(
        synth: Arc<Mutex<PianoSynthEngine>>,
        simulator: Arc<InputSimulator>,
        mapping: Arc<Mutex<KeyMappingEngine>>,
        config: Arc<Mutex<AppConfig>>,
    ) -> Self {
        Self {
            input_conn: Arc::new(Mutex::new(None)),
            output_conn: Arc::new(Mutex::new(None)),
            synth,
            simulator,
            mapping,
            config,
        }
    }

    /// List available MIDI input ports
    pub fn list_input_ports() -> Vec<String> {
        let Ok(midi_in) = MidiInput::new("vitl-piano-input-probe") else {
            return Vec::new();
        };
        let mut list = Vec::new();
        for port in midi_in.ports() {
            if let Ok(name) = midi_in.port_name(&port) {
                list.push(name);
            }
        }
        list
    }

    /// List available MIDI output ports
    pub fn list_output_ports() -> Vec<String> {
        let Ok(midi_out) = MidiOutput::new("vitl-piano-output-probe") else {
            return Vec::new();
        };
        let mut list = Vec::new();
        for port in midi_out.ports() {
            if let Ok(name) = midi_out.port_name(&port) {
                list.push(name);
            }
        }
        list
    }

    /// Connect to a hardware MIDI input port by name
    pub fn connect_input(&self, port_name: &str) -> Result<()> {
        let mut midi_in = MidiInput::new("vitl-piano-input")?;
        midi_in.ignore(midir::Ignore::None);

        let ports = midi_in.ports();
        let target_port = ports
            .into_iter()
            .find(|p| midi_in.port_name(p).map(|n| n == port_name).unwrap_or(false))
            .context("MIDI input port not found")?;

        let synth_arc = Arc::clone(&self.synth);
        let sim_arc = Arc::clone(&self.simulator);
        let map_arc = Arc::clone(&self.mapping);
        let cfg_arc = Arc::clone(&self.config);

        let conn = midi_in.connect(
            &target_port,
            "vitl-piano-in-handler",
            move |_timestamp, message, _| {
                if message.is_empty() {
                    return;
                }

                let status = message[0] & 0xF0;
                let cfg = cfg_arc.lock().clone();

                match status {
                    0x90 => {
                        // Note On
                        if message.len() >= 3 {
                            let note = message[1];
                            let velocity = message[2];

                            if velocity > 0 {
                                if cfg.synth.enabled {
                                    synth_arc.lock().note_on(note, velocity);
                                }
                                if cfg.macro_enabled {
                                    if cfg.velocity {
                                        let vel_char = map_arc.lock().get_velocity_key(velocity);
                                        sim_arc.send_velocity(vel_char);
                                    }
                                    if let Some(key_map) = map_arc.lock().get_piano_key(note, cfg.allow_88_keys) {
                                        let sim = Arc::clone(&sim_arc);
                                        tokio::task::spawn_blocking(move || {
                                            sim.tap_piano_key(key_map.key_char, key_map.is_shift, key_map.is_ctrl, 50);
                                        });
                                    }
                                }
                            } else {
                                if cfg.synth.enabled {
                                    synth_arc.lock().note_off(note);
                                }
                            }
                        }
                    }
                    0x80 => {
                        // Note Off
                        if message.len() >= 2 {
                            let note = message[1];
                            if cfg.synth.enabled {
                                synth_arc.lock().note_off(note);
                            }
                        }
                    }
                    0xB0 => {
                        // Control change
                        if message.len() >= 3 && message[1] == 64 {
                            let val = message[2];
                            let is_down = val > cfg.sustain_cutoff;
                            synth_arc.lock().set_sustain(is_down);
                            if cfg.macro_enabled && cfg.sustain {
                                sim_arc.set_sustain(is_down);
                            }
                        }
                    }
                    _ => {}
                }
            },
            (),
        ).map_err(|e| anyhow::anyhow!("Failed to connect MIDI input: {:?}", e))?;

        *self.input_conn.lock() = Some(conn);
        info!("Connected to MIDI input port '{}'", port_name);
        Ok(())
    }

    /// Disconnect MIDI input
    pub fn disconnect_input(&self) {
        *self.input_conn.lock() = None;
    }
}
