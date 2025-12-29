use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::core::ringbuffer::AudioRingBuffer;
use crate::core::timestamp::utc_ns_now;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeClass {
    Producer,
    Processor,
    Consumer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub class: NodeClass,
    pub node_type: String,
    pub config: serde_json::Value,
}

impl GraphNode {
    pub fn new(
        id: String,
        name: String,
        class: NodeClass,
        node_type: String,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id,
            name,
            class,
            node_type,
            config,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphConnection {
    pub id: String,
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
    pub buffer: Arc<AudioRingBuffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConnectionInfo {
    pub id: String,
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
}

impl GraphConnection {
    pub fn info(&self) -> GraphConnectionInfo {
        GraphConnectionInfo {
            id: self.id.clone(),
            source_node: self.source_node.clone(),
            source_port: self.source_port.clone(),
            target_node: self.target_node.clone(),
            target_port: self.target_port.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub nodes: Vec<GraphNode>,
    pub connections: Vec<GraphConnectionInfo>,
    pub running: bool,
}

pub struct AudioGraph {
    nodes: HashMap<String, GraphNode>,
    connections: HashMap<String, GraphConnection>,
    running: bool,
}

impl AudioGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            connections: HashMap::new(),
            running: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    pub fn add_node(&mut self, node: GraphNode) -> Result<String> {
        if self.nodes.contains_key(&node.id) {
            return Err(anyhow!("node '{}' already exists", node.id));
        }
        let id = node.id.clone();
        self.nodes.insert(node.id.clone(), node);
        Ok(id)
    }

    pub fn contains_node(&self, node_id: &str) -> bool {
        self.nodes.contains_key(node_id)
    }

    pub fn remove_node(&mut self, node_id: &str) -> Result<()> {
        if self.nodes.remove(node_id).is_none() {
            return Err(anyhow!("node '{}' not found", node_id));
        }

        self.connections
            .retain(|_, c| c.source_node != node_id && c.target_node != node_id);
        Ok(())
    }

    pub fn connect(
        &mut self,
        source_node: &str,
        source_port: &str,
        target_node: &str,
        target_port: &str,
    ) -> Result<String> {
        if !self.nodes.contains_key(source_node) {
            return Err(anyhow!("source node '{}' not found", source_node));
        }
        if !self.nodes.contains_key(target_node) {
            return Err(anyhow!("target node '{}' not found", target_node));
        }

        let connection_id = format!("conn-{}", utc_ns_now());
        let connection = GraphConnection {
            id: connection_id.clone(),
            source_node: source_node.to_string(),
            source_port: source_port.to_string(),
            target_node: target_node.to_string(),
            target_port: target_port.to_string(),
            buffer: Arc::new(AudioRingBuffer::new(1000)),
        };

        self.connections.insert(connection_id.clone(), connection);
        Ok(connection_id)
    }

    pub fn disconnect(&mut self, connection_id: &str) -> Result<()> {
        if self.connections.remove(connection_id).is_none() {
            return Err(anyhow!("connection '{}' not found", connection_id));
        }
        Ok(())
    }

    pub fn reconfigure_node(&mut self, node_id: &str, config: serde_json::Value) -> Result<()> {
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| anyhow!("node '{}' not found", node_id))?;
        node.config = config;
        Ok(())
    }

    pub fn snapshot(&self) -> GraphSnapshot {
        GraphSnapshot {
            nodes: self.nodes.values().cloned().collect(),
            connections: self.connections.values().map(|c| c.info()).collect(),
            running: self.running,
        }
    }

    pub fn validate_acyclic(&self) -> Result<()> {
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.visit(node_id, &mut visited, &mut stack)?;
            }
        }
        Ok(())
    }

    fn visit(
        &self,
        node_id: &str,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) -> Result<()> {
        visited.insert(node_id.to_string());
        stack.insert(node_id.to_string());

        for conn in self.connections.values() {
            if conn.source_node != node_id {
                continue;
            }

            let target = &conn.target_node;
            if stack.contains(target) {
                return Err(anyhow!("cycle detected: '{}' -> '{}'", node_id, target));
            }

            if !visited.contains(target) {
                self.visit(target, visited, stack)?;
            }
        }

        stack.remove(node_id);
        Ok(())
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}
