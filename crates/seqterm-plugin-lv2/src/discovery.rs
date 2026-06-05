//! Turn LV2 bundle TTL into structured plugin + port metadata.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use crate::ttl::{self, Graph, Node};

/// What a port carries and which direction it flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    AudioInput,
    AudioOutput,
    ControlInput,
    ControlOutput,
    AtomInput,
    AtomOutput,
    Unknown,
}

impl PortKind {
    pub fn is_audio(self) -> bool {
        matches!(self, PortKind::AudioInput | PortKind::AudioOutput)
    }
    pub fn is_control(self) -> bool {
        matches!(self, PortKind::ControlInput | PortKind::ControlOutput)
    }
    pub fn is_input(self) -> bool {
        matches!(
            self,
            PortKind::AudioInput | PortKind::ControlInput | PortKind::AtomInput
        )
    }
}

/// A single plugin port parsed from the TTL.
#[derive(Debug, Clone)]
pub struct Port {
    pub index: u32,
    pub symbol: String,
    pub name: String,
    pub kind: PortKind,
    /// True for an atom input port that accepts `midi:MidiEvent`.
    pub is_midi: bool,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

/// A discovered LV2 plugin: identity, binary, classification, and ports.
#[derive(Debug, Clone)]
pub struct Lv2PluginInfo {
    pub uri: String,
    pub name: String,
    pub bundle_dir: PathBuf,
    pub binary_path: PathBuf,
    pub is_instrument: bool,
    pub is_effect: bool,
    pub ports: Vec<Port>,
    /// Required-feature URIs declared by the plugin (we support only urid:map/unmap).
    pub required_features: Vec<String>,
}

/// Parse every plugin in `bundle_dir` (a `*.lv2` directory). Returns an empty
/// vec on any error (logged), so a bad bundle never aborts a scan.
pub fn discover_bundle(bundle_dir: &Path) -> Vec<Lv2PluginInfo> {
    let manifest = bundle_dir.join("manifest.ttl");
    let mgraph = match Graph::parse_file(&manifest) {
        Ok(g) => g,
        Err(e) => {
            debug!("LV2: no manifest in {}: {e}", bundle_dir.display());
            return Vec::new();
        }
    };

    let plugin_uris = mgraph.subjects_of_type(ttl::LV2_PLUGIN);
    if plugin_uris.is_empty() {
        return Vec::new();
    }

    // Build ONE combined graph for the whole bundle: the manifest plus every
    // distinct `rdfs:seeAlso` document, each parsed exactly once. (Multiple
    // plugins in a bundle typically share one big description `.ttl`.)
    let mut graph = Graph::default();
    graph.extend_from(&mgraph);
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for uri in &plugin_uris {
        for see in mgraph.objects(uri, ttl::RDFS_SEE_ALSO) {
            let Some(p) = node_to_path(see) else { continue };
            if seen.insert(p.clone())
                && let Ok(g) = Graph::parse_file(&p)
            {
                graph.extend_from(&g);
            }
        }
    }
    graph.index();

    let mut out = Vec::new();
    for uri in plugin_uris {
        let binary = graph.object(&uri, ttl::LV2_BINARY).and_then(node_to_path);
        let Some(binary_path) = binary else {
            warn!("LV2: plugin {uri} has no lv2:binary; skipping");
            continue;
        };
        let info = parse_plugin(&graph, &uri, bundle_dir, binary_path);
        out.push(info);
    }
    out
}

fn parse_plugin(graph: &Graph, uri: &str, bundle_dir: &Path, binary_path: PathBuf) -> Lv2PluginInfo {
    let name = graph
        .object(uri, ttl::DOAP_NAME)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| {
            // Fall back to the last URI path segment.
            uri.rsplit(['/', '#']).next().unwrap_or(uri).to_string()
        });

    let is_instrument = graph.has_type(uri, ttl::LV2_INSTRUMENT);

    let mut ports = Vec::new();
    for port_node in graph.objects(uri, ttl::LV2_PORT) {
        let pid = port_node.as_str();
        let port = parse_port(graph, pid);
        ports.push(port);
    }
    ports.sort_by_key(|p| p.index);

    let has_audio_out = ports.iter().any(|p| p.kind == PortKind::AudioOutput);
    let has_audio_in = ports.iter().any(|p| p.kind == PortKind::AudioInput);
    // An effect processes audio in→out; an instrument has audio out + MIDI in.
    let is_effect = has_audio_in && has_audio_out && !is_instrument;

    let required_features = graph
        .objects(uri, ttl::LV2_REQUIRED_FEATURE)
        .iter()
        .map(|n| n.as_str().to_string())
        .collect();

    Lv2PluginInfo {
        uri: uri.to_string(),
        name,
        bundle_dir: bundle_dir.to_path_buf(),
        binary_path,
        is_instrument,
        is_effect,
        ports,
        required_features,
    }
}

fn parse_port(graph: &Graph, pid: &str) -> Port {
    let index = graph
        .object(pid, ttl::LV2_INDEX)
        .and_then(|n| n.as_str().parse::<u32>().ok())
        .unwrap_or(u32::MAX);
    let symbol = graph
        .object(pid, ttl::LV2_SYMBOL)
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let name = graph
        .object(pid, ttl::LV2_NAME)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| symbol.clone());

    let is_input = graph.has_type(pid, ttl::LV2_INPUT_PORT);
    let is_output = graph.has_type(pid, ttl::LV2_OUTPUT_PORT);
    let is_audio = graph.has_type(pid, ttl::LV2_AUDIO_PORT);
    let is_control = graph.has_type(pid, ttl::LV2_CONTROL_PORT);
    let is_atom = graph.has_type(pid, ttl::ATOM_PORT);

    let kind = match (is_audio, is_control, is_atom, is_input, is_output) {
        (true, _, _, true, _) => PortKind::AudioInput,
        (true, _, _, _, true) => PortKind::AudioOutput,
        (_, true, _, true, _) => PortKind::ControlInput,
        (_, true, _, _, true) => PortKind::ControlOutput,
        (_, _, true, true, _) => PortKind::AtomInput,
        (_, _, true, _, true) => PortKind::AtomOutput,
        _ => PortKind::Unknown,
    };

    let is_midi = is_atom
        && graph
            .objects(pid, ttl::ATOM_SUPPORTS)
            .iter()
            .any(|n| n.as_str() == ttl::MIDI_EVENT);

    let default = graph
        .object(pid, ttl::LV2_DEFAULT)
        .and_then(|n| n.as_str().parse::<f32>().ok())
        .unwrap_or(0.0);
    let min = graph
        .object(pid, ttl::LV2_MINIMUM)
        .and_then(|n| n.as_str().parse::<f32>().ok())
        .unwrap_or(0.0);
    let max = graph
        .object(pid, ttl::LV2_MAXIMUM)
        .and_then(|n| n.as_str().parse::<f32>().ok())
        .unwrap_or(1.0);

    Port {
        index,
        symbol,
        name,
        kind,
        is_midi,
        default,
        min,
        max,
    }
}

/// Resolve a TTL object node into a filesystem path (it is a resolved `file://`
/// IRI when the source was a relative `<…>` ref).
fn node_to_path(node: &Node) -> Option<PathBuf> {
    match node {
        Node::Iri(iri) => ttl::file_uri_to_path(iri),
        _ => None,
    }
}
