use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::core::graph::{AudioGraph, GraphNode, NodeClass};
use crate::core::timestamp::utc_ns_now;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRequest {
    pub name: String,
    pub class: NodeClass,
    pub node_type: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRequest {
    pub source_node: String,
    pub source_port: String,
    pub target_node: String,
    pub target_port: String,
}

#[derive(Debug, Clone)]
pub enum DisconnectStrategy {
    DropConnections,
    KeepBuffers,
}

pub struct GraphApi {
    graph: AudioGraph,
}

impl GraphApi {
    pub fn new() -> Self {
        Self {
            graph: AudioGraph::new(),
        }
    }

    pub fn graph(&self) -> &AudioGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut AudioGraph {
        &mut self.graph
    }

    pub fn add_node(&mut self, request: NodeRequest) -> Result<String> {
        let node_id = format!("node-{}", utc_ns_now());
        let node = GraphNode::new(
            node_id.clone(),
            request.name,
            request.class,
            request.node_type,
            request.config,
        );
        self.graph.add_node(node)?;
        Ok(node_id)
    }

    pub fn remove_node(&mut self, node_id: &str, _strategy: DisconnectStrategy) -> Result<()> {
        self.graph.remove_node(node_id)
    }

    pub fn connect(&mut self, request: ConnectionRequest) -> Result<String> {
        self.graph.connect(
            &request.source_node,
            &request.source_port,
            &request.target_node,
            &request.target_port,
        )
    }

    pub fn disconnect(&mut self, connection_id: &str) -> Result<()> {
        self.graph.disconnect(connection_id)
    }

    pub fn reconfigure_node(&mut self, node_id: &str, config: serde_json::Value) -> Result<()> {
        self.graph.reconfigure_node(node_id, config)
    }

    pub fn replace_node(&mut self, node_id: &str, request: NodeRequest) -> Result<String> {
        if !self.graph.contains_node(node_id) {
            return Err(anyhow!("node '{}' not found", node_id));
        }
        self.remove_node(node_id, DisconnectStrategy::DropConnections)?;
        self.add_node(request)
    }
}

impl Default for GraphApi {
    fn default() -> Self {
        Self::new()
    }
}
