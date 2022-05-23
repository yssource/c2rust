use crate::graph::{Graph, GraphId, Graphs, Node, NodeId, NodeKind};
use bincode;
use c2rust_analysis_rt::events::{Event, EventKind};
use c2rust_analysis_rt::mir_loc::{EventMetadata, Metadata, RefKind};
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
fn get_src_ref(kind: &EventKind, metadata: &EventMetadata) -> Option<RefKind> {
    Some(match kind {
        EventKind::CopyPtr(ptr) => {
            RefKind::Raw(*ptr)
            // return metadata.source()
        }
        EventKind::Field(ptr, ..) => RefKind::Raw(*ptr),
        EventKind::Alloc { ptr, .. } => RefKind::Raw(*ptr),
        EventKind::Free { ptr } => RefKind::Raw(*ptr),
        EventKind::Realloc { old_ptr, .. } => RefKind::Raw(*old_ptr),
        EventKind::Arg(ptr) => RefKind::Raw(*ptr),
        EventKind::Ret(ptr) => RefKind::Raw(*ptr),
        EventKind::Done => return None,
        EventKind::LoadAddr(ptr) => RefKind::Raw(*ptr),
        EventKind::StoreAddr(ptr) => RefKind::Raw(*ptr),
        EventKind::CopyRef => {
            return metadata.source();
        },
    })
}

pub fn event_to_node_kind(event: &Event) -> Option<NodeKind> {
    Some(match event.kind {
        EventKind::Alloc { .. } => NodeKind::Malloc(1),
        EventKind::Realloc { .. } => NodeKind::Malloc(1),
        EventKind::Free { .. } => NodeKind::Free,
        EventKind::CopyPtr(..) | EventKind::CopyRef => NodeKind::Copy,
        EventKind::Field(_, field) => NodeKind::Field(field.into()),
        EventKind::LoadAddr(..) => NodeKind::LoadAddr,
        EventKind::StoreAddr(..) => NodeKind::StoreAddr,
        EventKind::Arg(_) => NodeKind::Arg,
        _ => return None,
    })
}

pub fn handle_provenance(
    provenances: &mut HashMap<RefKind, (GraphId, NodeId)>,
    event_kind: &EventKind,
    metadata: &EventMetadata,
    mapping: (GraphId, NodeId),
) {
    match event_kind {
        EventKind::Alloc { ptr, .. } => {
            provenances.insert(RefKind::Raw(*ptr), mapping);
            // provenances.insert(metadata.dest().unwrap(), mapping);
        }
        EventKind::Realloc { new_ptr, .. } => {
            provenances.insert(RefKind::Raw(*new_ptr), mapping);
            // provenances.insert(metadata.dest().unwrap(), mapping);
        }
        EventKind::CopyPtr(dst) => {
            // provenances.insert(metadata.dest().unwrap(), mapping);
            provenances.insert(RefKind::Raw(*dst), mapping);
        }
        EventKind::CopyRef => {
            provenances.insert(metadata.dest().unwrap(), mapping);
        }
        _ => (),
    }
}

pub fn add_node(
    graphs: &mut Graphs,
    provenances: &mut HashMap<RefKind, (GraphId, NodeId)>,
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
        metadata,
    } = mir_loc::get(event.mir_loc).unwrap();

    let source = get_src_ref(&event.kind, &metadata)
        .and_then(|p| {
            provenances.get(&p)
        })
        .cloned();

    let store = match metadata {
        EventMetadata::CopyPtr(dest, _src) => Some(*dest),
        EventMetadata::CopyRef(dest, _src) => Some(*dest),
        _ => None,
    };

    println!("metadata: {:?}", metadata);

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

    handle_provenance(provenances, &event.kind, metadata, (graph_id, node_id));

    Some(node_id)
}

pub fn construct_pdg(events: &Vec<Event>) -> Graphs {
    let mut graphs = Graphs::new();
    let mut provenances = HashMap::<RefKind, (GraphId, NodeId)>::new();
    for event in events {
        add_node(&mut graphs, &mut provenances, event);
    }

    graphs
}
