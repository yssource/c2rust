use crate::graph::{Graph, GraphId, Graphs, Node, NodeId, NodeKind};
use bincode;
use c2rust_analysis_rt::events::{Event, EventKind};
use c2rust_analysis_rt::mir_loc::Metadata;
use c2rust_analysis_rt::{mir_loc, MirLoc};
use rustc_data_structures::fingerprint::Fingerprint;
use rustc_hir::def_id::DefPathHash;
use rustc_middle::mir::{Field, Local};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

pub fn read_event_log(path: String) -> Vec<Event> {
    let file = File::open(path).unwrap();
    let mut events = vec![];
    let mut reader = BufReader::new(file);
    loop {
        match bincode::deserialize_from(&mut reader) {
            Ok(e) => events.push(e),
            _ => break,
        }
    }
    events
}

pub fn read_metadata(path: String) -> Metadata {
    let file = File::open(path).unwrap();
    bincode::deserialize_from(file).unwrap()
}

/** return the ptr referenced by an EventKind */
fn get_ptr(kind: &EventKind) -> Option<&usize> {
    Some(match kind {
        EventKind::Copy(ptr) => ptr,
        EventKind::Field(ptr, ..) => ptr,
        EventKind::Alloc { ptr, .. } => ptr,
        EventKind::Free { ptr } => ptr,
        EventKind::Realloc { old_ptr, .. } => old_ptr,
        EventKind::Arg(ptr) => ptr,
        EventKind::Ret(ptr) => ptr,
        EventKind::Done => return None,
        EventKind::LoadAddr(ptr) => ptr,
        EventKind::StoreAddr(ptr) => ptr,
        EventKind::CopyLocal => return None,
    })
}

/** return the new ptr created by an EventKind */
fn get_new_ptr(kind: &EventKind) -> Option<&usize> {
    Some(match kind {
        // EventKind::Field(ptr, id) => todo!("ptr + id to offset"),
        EventKind::Alloc { ptr, .. } => ptr,
        EventKind::Realloc { new_ptr, .. } => new_ptr,
        _ => return None,
    })
}

pub fn event_to_node_kind(event: &Event) -> Option<NodeKind> {
    Some(match event.kind {
        EventKind::Alloc { .. } => NodeKind::Malloc(1),
        EventKind::Realloc { .. } => NodeKind::Malloc(1),
        EventKind::Free { .. } => NodeKind::Free,
        EventKind::Copy(..) | EventKind::CopyLocal => NodeKind::Copy,
        EventKind::Field(_, field) => NodeKind::Field(field.into()),
        EventKind::LoadAddr(..) => NodeKind::LoadAddr,
        EventKind::StoreAddr(..) => NodeKind::StoreAddr,
        _ => return None,
    })
}

pub fn add_node(
    graphs: &mut Graphs,
    provenances: &mut HashMap<usize, (GraphId, NodeId)>,
    event: &Event,
) -> Option<NodeId> {
    let node_kind = match event_to_node_kind(event) {
        Some(kind) => kind,
        None => return None,
    };

    let MirLoc {
        body_def,
        basic_block_idx,
        statement_idx,
        store,
    } = mir_loc::get(event.mir_loc).unwrap();

    let source = get_ptr(&event.kind)
        .and_then(|p| provenances.get(p))
        .cloned();

    let node = Node {
        function: DefPathHash(Fingerprint::new(body_def.0, body_def.1).into()),
        block: basic_block_idx.clone().into(),
        index: statement_idx.clone().into(),
        kind: node_kind,
        source: source.map(|(_, nid)| nid),
        dest: store.map(Local::from),
    };

    let graph_id = source
        .map(|(gid, _)| gid)
        .unwrap_or_else(|| graphs.graphs.push(Graph::new()));
    let node_id = graphs.graphs[graph_id].nodes.push(node);

    if let Some(&ptr) = get_new_ptr(&event.kind) {
        provenances.insert(ptr, (graph_id, node_id));
    }
    Some(node_id)
}

pub fn construct_pdg(events: &Vec<Event>) -> Graphs {
    let mut graphs = Graphs::new();
    let mut provenances = HashMap::<usize, (GraphId, NodeId)>::new();
    for event in events {
        add_node(&mut graphs, &mut provenances, event);
    }

    graphs
}
