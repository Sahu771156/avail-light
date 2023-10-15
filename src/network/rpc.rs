use anyhow::Result;
use avail_subxt::utils::H256;
use kate_recovery::matrix::{Dimensions, Position};
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::{collections::HashSet, fmt::Display};
use tokio::sync::{broadcast, mpsc};
use tracing::debug;

use crate::consts::EXPECTED_NETWORK_VERSION;

use self::{client::Client, event_loop::EventLoop};

mod client;
mod event_loop;

const CELL_SIZE: usize = 32;
const PROOF_SIZE: usize = 48;
pub const CELL_WITH_PROOF_SIZE: usize = CELL_SIZE + PROOF_SIZE;

#[derive(Clone)]
pub struct Node {
	pub host: String,
	pub system_version: String,
	pub spec_version: u32,
	pub genesis_hash: H256,
}

impl Node {
	pub fn network(&self) -> String {
		format!(
			"{host}/{system_version}/{spec_name}/{spec_version}",
			host = self.host,
			system_version = self.system_version,
			spec_name = EXPECTED_NETWORK_VERSION.spec_name,
			spec_version = self.spec_version,
		)
	}
}

pub struct Nodes {
	list: Vec<Node>,
	current_index: usize,
}

impl Nodes {
	pub fn next(&mut self) -> Option<Node> {
		// we have exhausted all nodes from the list
		// this is the last one
		if self.current_index == self.list.len() - 1 {
			None
		} else {
			// increment current index
			self.current_index += 1;
			self.get_current()
		}
	}

	pub fn get_current(&self) -> Option<Node> {
		let node = &self.list[self.current_index];
		Some(node.clone())
	}

	pub fn init(&mut self, nodes: &[String], last_known_node: Option<String>) -> Self {
		let mut candidates = nodes.to_owned();
		candidates.retain(|node| Some(node) != last_known_node.as_ref());

		Self {
			list: candidates
				.iter()
				.map(|s| Node {
					genesis_hash: Default::default(),
					spec_version: Default::default(),
					system_version: Default::default(),
					host: s.to_string(),
				})
				.collect(),
			current_index: 0,
		}
	}

	fn shuffle(&mut self) {
		self.list.shuffle(&mut thread_rng());
	}

	fn reset(&mut self) -> Option<Node> {
		// shuffle the available list of nodes
		self.shuffle();
		// set the current index to the first one
		self.current_index = 0;
		self.get_current()
	}
}

#[derive(Debug)]
pub struct ExpectedVersion<'a> {
	pub version: &'a str,
	pub spec_name: &'a str,
}

impl ExpectedVersion<'_> {
	/// Checks if expected version matches network version.
	/// Since the light client uses subset of the node APIs, `matches` checks only prefix of a node version.
	/// This means that if expected version is `1.6`, versions `1.6.x` of the node will match.
	/// Specification name is checked for exact match.
	/// Since runtime `spec_version` can be changed with runtime upgrade, `spec_version` is removed.
	/// NOTE: Runtime compatibility check is currently not implemented.
	pub fn matches(&self, node_version: &str, spec_name: &str) -> bool {
		node_version.starts_with(self.version) && self.spec_name == spec_name
	}
}

impl Display for ExpectedVersion<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "v{}/{}", self.version, self.spec_name)
	}
}

pub fn init(nodes: Nodes) -> Result<(Client, EventLoop)> {
	// create sender channel for Event Loop Commands
	let (command_sender, command_receiver) = mpsc::channel(1000);
	let (event_sender, event_receiver) = broadcast::channel(1000);

	Ok((
		Client::new(command_sender),
		EventLoop::new(nodes, command_receiver, event_sender),
	))
}

/// Generates random cell positions for sampling
pub fn generate_random_cells(dimensions: Dimensions, cell_count: u32) -> Vec<Position> {
	let max_cells = dimensions.extended_size();
	let count = if max_cells < cell_count {
		debug!("Max cells count {max_cells} is lesser than cell_count {cell_count}");
		max_cells
	} else {
		cell_count
	};
	let mut rng = thread_rng();
	let mut indices = HashSet::new();
	while (indices.len() as u16) < count as u16 {
		let col = rng.gen_range(0..dimensions.cols().into());
		let row = rng.gen_range(0..dimensions.extended_rows());
		indices.insert(Position { row, col });
	}

	indices.into_iter().collect::<Vec<_>>()
}

/* @note: fn to take the number of cells needs to get equal to or greater than
the percentage of confidence mentioned in config file */

/// Calculates number of cells required to achieve given confidence
pub fn cell_count_for_confidence(confidence: f64) -> u32 {
	let mut cell_count: u32;
	if !(50.0..100f64).contains(&confidence) {
		//in this default of 8 cells will be taken
		debug!(
			"confidence is {} invalid so taking default confidence of 99",
			confidence
		);
		cell_count = (-((1f64 - (99f64 / 100f64)).log2())).ceil() as u32;
	} else {
		cell_count = (-((1f64 - (confidence / 100f64)).log2())).ceil() as u32;
	}
	if cell_count == 0 || cell_count > 10 {
		debug!(
			"confidence is {} invalid so taking default confidence of 99",
			confidence
		);
		cell_count = (-((1f64 - (99f64 / 100f64)).log2())).ceil() as u32;
	}
	cell_count
}