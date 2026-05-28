use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// A node in the MIDI routing graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RoutingNode {
    PatternOut { key: String },
    MidiIn     { port: String },
    MidiOut    { port: String },
    AudioBus   { id: u8, name: String },
    Send       { id: u8, name: String },
}

impl RoutingNode {
    pub fn label(&self) -> String {
        match self {
            RoutingNode::PatternOut { key }      => format!("PAT: {key}"),
            RoutingNode::MidiIn { port }         => format!("IN:  {port}"),
            RoutingNode::MidiOut { port }        => format!("OUT: {port}"),
            RoutingNode::AudioBus { id, name }   => format!("BUS{id}: {name}"),
            RoutingNode::Send { id, name }       => format!("SND{id}: {name}"),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            RoutingNode::PatternOut { .. } => "pattern",
            RoutingNode::MidiIn { .. }     => "midi-in",
            RoutingNode::MidiOut { .. }    => "midi-out",
            RoutingNode::AudioBus { .. }   => "bus",
            RoutingNode::Send { .. }       => "send",
        }
    }
}

/// A directed connection between two routing nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingEdge {
    pub from: u32,
    pub to:   u32,
    /// Per-channel remap: `None` = pass through, `Some(ch)` = force to channel.
    pub channel_map: [Option<u8>; 16],
}

impl RoutingEdge {
    pub fn new(from: u32, to: u32) -> Self {
        Self { from, to, channel_map: [None; 16] }
    }
}

/// Graph of MIDI routing nodes and directed edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutingGraph {
    pub nodes:   HashMap<u32, RoutingNode>,
    pub edges:   Vec<RoutingEdge>,
    pub next_id: u32,
}

impl RoutingGraph {
    pub fn add_node(&mut self, node: RoutingNode) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(id, node);
        id
    }

    pub fn remove_node(&mut self, id: u32) {
        self.nodes.remove(&id);
        self.edges.retain(|e| e.from != id && e.to != id);
    }

    pub fn add_edge(&mut self, from: u32, to: u32) -> bool {
        if !self.nodes.contains_key(&from) || !self.nodes.contains_key(&to) {
            return false;
        }
        if self.edges.iter().any(|e| e.from == from && e.to == to) {
            return false;
        }
        if self.has_cycle_with(from, to) {
            return false;
        }
        self.edges.push(RoutingEdge::new(from, to));
        true
    }

    pub fn remove_edge(&mut self, from: u32, to: u32) {
        self.edges.retain(|e| !(e.from == from && e.to == to));
    }

    pub fn has_edge(&self, from: u32, to: u32) -> bool {
        self.edges.iter().any(|e| e.from == from && e.to == to)
    }

    pub fn outgoing(&self, id: u32) -> Vec<u32> {
        self.edges.iter().filter(|e| e.from == id).map(|e| e.to).collect()
    }

    pub fn incoming(&self, id: u32) -> Vec<u32> {
        self.edges.iter().filter(|e| e.to == id).map(|e| e.from).collect()
    }

    fn has_cycle_with(&self, from: u32, to: u32) -> bool {
        let mut visited = HashSet::new();
        self.dfs_reachable(to, from, &mut visited)
    }

    fn dfs_reachable(&self, start: u32, target: u32, visited: &mut HashSet<u32>) -> bool {
        if start == target { return true; }
        if !visited.insert(start) { return false; }
        self.edges.iter()
            .filter(|e| e.from == start)
            .any(|e| self.dfs_reachable(e.to, target, visited))
    }

    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut stack   = HashSet::new();
        self.nodes.keys().any(|&id| self.dfs_cycle(id, &mut visited, &mut stack))
    }

    fn dfs_cycle(&self, node: u32, visited: &mut HashSet<u32>, stack: &mut HashSet<u32>) -> bool {
        if stack.contains(&node) { return true; }
        if visited.contains(&node) { return false; }
        visited.insert(node);
        stack.insert(node);
        let cycle = self.edges.iter()
            .filter(|e| e.from == node)
            .any(|e| self.dfs_cycle(e.to, visited, stack));
        stack.remove(&node);
        cycle
    }

    /// Sorted list of node IDs, stable for display.
    pub fn sorted_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.nodes.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Clone into a lightweight snapshot the scheduler can hold without locking.
    pub fn realtime_snapshot(&self) -> RoutingSnapshot {
        RoutingSnapshot {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
        }
    }
}

/// A lock-free read-only snapshot of the routing graph.
#[derive(Debug, Clone, Default)]
pub struct RoutingSnapshot {
    pub nodes: HashMap<u32, RoutingNode>,
    pub edges: Vec<RoutingEdge>,
}

impl RoutingSnapshot {
    pub fn outgoing(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        self.edges.iter().filter(move |e| e.from == id).map(|e| e.to)
    }

    pub fn edges_from(&self, id: u32) -> Vec<&RoutingEdge> {
        self.edges.iter().filter(|e| e.from == id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(key: &str) -> RoutingNode { RoutingNode::PatternOut { key: key.into() } }
    fn midi_out(port: &str) -> RoutingNode { RoutingNode::MidiOut { port: port.into() } }

    #[test]
    fn add_and_remove_node() {
        let mut g = RoutingGraph::default();
        let id = g.add_node(pat("A0"));
        assert!(g.nodes.contains_key(&id));
        g.remove_node(id);
        assert!(!g.nodes.contains_key(&id));
    }

    #[test]
    fn add_edge_and_detect() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A0"));
        let b = g.add_node(midi_out("SYNTH"));
        assert!(g.add_edge(a, b));
        assert!(g.has_edge(a, b));
    }

    #[test]
    fn no_cycle_linear() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A0"));
        let b = g.add_node(midi_out("B"));
        let c = g.add_node(midi_out("C"));
        assert!(g.add_edge(a, b));
        assert!(g.add_edge(b, c));
        assert!(!g.has_cycle());
    }

    #[test]
    fn cycle_detected() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        let c = g.add_node(midi_out("C"));
        assert!(g.add_edge(a, b));
        assert!(g.add_edge(b, c));
        // Creating C→A would close a cycle — has_cycle_with should detect it.
        assert!(g.has_cycle_with(c, a));
    }

    #[test]
    fn adding_cycle_edge_is_rejected() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        assert!(g.add_edge(a, b));
        // Adding b→a would create a cycle; the graph should return false.
        assert!(!g.add_edge(b, a));
    }

    #[test]
    fn remove_node_prunes_edges() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        assert!(g.add_edge(a, b));
        g.remove_node(a);
        assert!(!g.has_edge(a, b));
        assert!(g.edges.is_empty());
    }

    #[test]
    fn snapshot_is_independent() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        assert!(g.add_edge(a, b));
        let snap = g.realtime_snapshot();
        // Mutate original — snapshot should be unaffected.
        g.remove_node(a);
        assert!(snap.nodes.contains_key(&a));
    }

    #[test]
    fn outgoing_edges() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        let c = g.add_node(midi_out("C"));
        assert!(g.add_edge(a, b));
        assert!(g.add_edge(a, c));
        let outs = g.outgoing(a);
        assert_eq!(outs.len(), 2);
    }

    #[test]
    fn duplicate_edge_is_noop() {
        let mut g = RoutingGraph::default();
        let a = g.add_node(pat("A"));
        let b = g.add_node(midi_out("B"));
        assert!(g.add_edge(a, b));
        // Adding the same edge again should not duplicate it.
        assert!(!g.add_edge(a, b));
        assert_eq!(g.outgoing(a).len(), 1);
    }
}
