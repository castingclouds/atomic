use crate::alive::{Graph, VertexId};
use crate::change::*;
use crate::pristine::*;
use crate::{HashMap, HashSet};
use std::collections::hash_map::Entry;

#[derive(Debug, Error)]
pub enum MissingError<TxnError: std::error::Error + 'static> {
    #[error(transparent)]
    Txn(TxnError),
    #[error(transparent)]
    Block(#[from] BlockError<TxnError>),
    #[error(transparent)]
    Inconsistent(#[from] InconsistentChange<TxnError>),
}

impl<T: std::error::Error + 'static> std::convert::From<TxnErr<T>> for MissingError<T> {
    fn from(e: TxnErr<T>) -> Self {
        MissingError::Txn(e.0)
    }
}

impl Workspace {
    pub(crate) fn load_graph<T: GraphTxnT>(
        &mut self,
        txn: &T,
        channel: &T::Graph,
        inode: Position<Option<Hash>>,
    ) -> Result<
        Option<&(Graph, HashMap<Vertex<NodeId>, VertexId>)>,
        InconsistentChange<T::GraphError>,
    > {
        if let Some(change) = inode.change {
            match self.graphs.0.entry(inode) {
                Entry::Occupied(e) => Ok(Some(e.into_mut())),
                Entry::Vacant(v) => {
                    let pos = Position {
                        change: if let Some(&i) = txn.get_internal(&change.into())? {
                            i
                        } else {
                            return Err(InconsistentChange::UndeclaredDep);
                        },
                        pos: inode.pos,
                    };
                    let mut graph = crate::alive::retrieve(txn, channel, pos, false)?;
                    graph.tarjan();
                    let mut ids = HashMap::default();
                    for (i, l) in graph.lines.iter().enumerate() {
                        ids.insert(l.vertex, VertexId(i));
                    }
                    Ok(Some(v.insert((graph, ids))))
                }
            }
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn has_missing_context_nondeleted<T: GraphMutTxnT>(
    txn: &T,
    channel: &T::Graph,
    change_id: NodeId,
    e: &NewEdge<Option<Hash>>,
) -> Result<bool, MissingError<T::GraphError>> {
    if e.flag.contains(EdgeFlags::FOLDER) {
        return Ok(false);
    }
    let source = *txn.find_block_end(&channel, internal_pos(txn, &e.from, change_id)?)?;
    let target = *txn.find_block(&channel, internal_pos(txn, &e.to.start_pos(), change_id)?)?;
    debug!(
        "repair_context_nondeleted source {:?} target {:?} e {:?}",
        source, target, e
    );
    if is_alive(txn, channel, &source)? && e.flag.contains(EdgeFlags::BLOCK) {
        Ok(iter_adjacent(
            txn,
            channel,
            target,
            EdgeFlags::DELETED,
            EdgeFlags::all() - EdgeFlags::PARENT,
        )?
        .next()
        .is_some())
    } else {
        Ok(true)
    }
}

pub(crate) fn has_missing_context_deleted<T: GraphMutTxnT, K>(
    txn: &T,
    channel: &T::Graph,
    change_id: NodeId,
    mut known: K,
    e: &NewEdge<Option<Hash>>,
) -> Result<bool, MissingError<T::GraphError>>
where
    K: FnMut(Hash) -> bool,
{
    if e.flag.contains(EdgeFlags::FOLDER) {
        return Ok(false);
    }
    debug!("repair_context_deleted {:?}", e);
    let mut pos = internal_pos(txn, &e.to.start_pos(), change_id)?;
    while let Ok(&dest_vertex) = txn.find_block(&channel, pos) {
        debug!("repair_context_deleted, dest_vertex = {:?}", dest_vertex);

        if has_unknown_children(txn, channel, dest_vertex, change_id, &mut known)? {
            return Ok(true);
        }

        if dest_vertex.end < e.to.end {
            pos.pos = dest_vertex.end
        } else {
            break;
        }
    }
    Ok(false)
}

/// If we're deleting a folder edge, there is a possibility that this
/// solves a "zombie conflict", by removing the last child of a folder
/// that was a zombie, in which case that parent folder can finally
/// rest in peace.
///
/// This function takes care of this for the entire change, by
/// removing all obsolete folder conflict edges.
pub(crate) fn detect_folder_conflict_resolutions<T: GraphMutTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    ws: &mut Workspace,
    change_id: NodeId,
    change: &Change,
) -> Result<(), MissingError<T::GraphError>> {
    for change_ in change.changes.iter() {
        for change_ in change_.iter() {
            if let Atom::EdgeMap(ref n) = *change_ {
                for edge in n.edges.iter() {
                    if !edge.flag.contains(EdgeFlags::DELETED) {
                        continue;
                    }
                    detect_folder_conflict_resolution(txn, channel, ws, change_id, &n.inode, edge)?
                }
            }
        }
    }
    Ok(())
}

fn detect_folder_conflict_resolution<T: GraphMutTxnT>(
    txn: &mut T,
    channel: &mut T::Graph,
    ws: &mut Workspace,
    change_id: NodeId,
    inode: &Position<Option<Hash>>,
    e: &NewEdge<Option<Hash>>,
) -> Result<(), MissingError<T::GraphError>> {
    let mut stack = vec![if e.flag.contains(EdgeFlags::FOLDER) {
        if e.to.is_empty() {
            internal_pos(txn, &e.to.start_pos(), change_id)?
        } else {
            internal_pos(txn, &e.from, change_id)?
        }
    } else {
        internal_pos(txn, &inode, change_id)?
    }];
    let len = ws.pseudo.len();
    while let Some(pos) = stack.pop() {
        let dest_vertex = if let Ok(&dest_vertex) = txn.find_block_end(&channel, pos) {
            if !dest_vertex.is_empty() {
                continue;
            }
            dest_vertex
        } else {
            continue;
        };
        // Is `dest_vertex` alive? If so, stop this path.
        let f0 = EdgeFlags::FOLDER | EdgeFlags::PARENT;
        let f1 = EdgeFlags::FOLDER | EdgeFlags::PARENT | EdgeFlags::BLOCK;
        if let Some(e) = iter_adjacent(txn, channel, dest_vertex, f0, f1)?
            .filter_map(|e| e.ok())
            .filter(|e| !e.flag().contains(EdgeFlags::PSEUDO))
            .next()
        {
            debug!("is_alive: {:?}", e);
            continue;
        }
        // Does `dest_vertex` have alive or zombie descendants? If
        // so, stop this path.
        let f0 = EdgeFlags::empty();
        let f1 = EdgeFlags::FOLDER | EdgeFlags::BLOCK | EdgeFlags::PSEUDO;
        if let Some(e) = iter_adjacent(txn, channel, dest_vertex, f0, f1)?.next() {
            debug!("child is_alive: {:?}", e);
            continue;
        }
        // Else, `dest_vertex` is dead. We should remove its
        // pseudo-parents.
        let f = EdgeFlags::FOLDER | EdgeFlags::PARENT | EdgeFlags::PSEUDO;
        for e in iter_adjacent(txn, channel, dest_vertex, f, f)? {
            let e = e?;
            ws.pseudo.push((dest_vertex, *e));
            let gr = *txn.find_block_end(&channel, e.dest()).unwrap();
            for e in iter_adjacent(txn, channel, gr, f, f)? {
                let e = e?;
                ws.pseudo.push((gr, *e));
                stack.push(e.dest())
            }
        }
    }
    // Finally, we were only squatting the `.pseudo` field of the
    // workspace, since that field is normally used for missing
    // children, and pseudo-edges from that field are treated in a
    // special way (by `delete_pseudo_edges` in this module).
    //
    // Therefore, we need to delete our folder-pseudo-edges now.
    for (v, e) in ws.pseudo.drain(len..) {
        let p = *txn.find_block_end(channel, e.dest())?;
        debug!(
            "detect folder conflict resolution, deleting {:?} → {:?} {:?}",
            v, e, p
        );
        del_graph_with_rev(
            txn,
            channel,
            e.flag() - EdgeFlags::PARENT,
            p,
            v,
            e.introduced_by(),
        )?;
    }
    Ok(())
}

#[derive(Default)]
pub struct Workspace {
    pub(crate) unknown_parents: Vec<(
        Vertex<NodeId>,
        Vertex<NodeId>,
        Position<Option<Hash>>,
        EdgeFlags,
    )>,
    unknown: Vec<SerializedEdge>,
    pub(crate) parents: HashSet<SerializedEdge>,
    pub(crate) pseudo: Vec<(Vertex<NodeId>, SerializedEdge)>,
    repaired: HashSet<Vertex<NodeId>>,
    pub(crate) graphs: Graphs,
    pub(crate) covered_parents: HashSet<(Vertex<NodeId>, Vertex<NodeId>)>,
    pub(crate) files: HashSet<Vertex<NodeId>>,
    alive_down_cache: HashMap<Vertex<NodeId>, Option<HashSet<Vertex<NodeId>>>>,
    alive_up_cache:
        HashMap<Vertex<NodeId>, (Option<HashSet<Vertex<NodeId>>>, HashSet<Vertex<NodeId>>)>,
    missing_down: Vec<Vertex<NodeId>>,
}

#[derive(Debug, Default)]
pub(crate) struct Graphs(
    pub HashMap<Position<Option<Hash>>, (Graph, HashMap<Vertex<NodeId>, crate::alive::VertexId>)>,
);

impl Graphs {
    pub(crate) fn get(
        &self,
        inode: Position<Option<Hash>>,
    ) -> Option<&(Graph, HashMap<Vertex<NodeId>, VertexId>)> {
        self.0.get(&inode)
    }

    pub fn split(
        &mut self,
        inode: Position<Option<Hash>>,
        vertex: Vertex<NodeId>,
        mid: ChangePosition,
    ) {
        if let Some((_, vids)) = self.0.get_mut(&inode) {
            if let Some(vid) = vids.remove(&vertex) {
                vids.insert(Vertex { end: mid, ..vertex }, vid);
                vids.insert(
                    Vertex {
                        start: mid,
                        ..vertex
                    },
                    vid,
                );
            }
        }
    }
}

impl Workspace {
    pub fn clear(&mut self) {
        self.unknown.clear();
        self.unknown_parents.clear();
        self.pseudo.clear();
        self.parents.clear();
        self.graphs.0.clear();
        self.repaired.clear();
        self.covered_parents.clear();
        self.alive_up_cache.clear();
        self.alive_down_cache.clear();
        self.missing_down.clear();
    }
    pub fn assert_empty(&self) {
        assert!(self.unknown.is_empty());
        assert!(self.unknown_parents.is_empty());
        assert!(self.pseudo.is_empty());
        assert!(self.parents.is_empty());
        assert!(self.graphs.0.is_empty());
        assert!(self.repaired.is_empty());
        assert!(self.covered_parents.is_empty());
        assert!(self.alive_up_cache.is_empty());
        assert!(self.alive_down_cache.is_empty());
        assert!(self.missing_down.is_empty());
    }
}

fn has_unknown_children<T: GraphTxnT, K>(
    txn: &T,
    channel: &T::Graph,
    dest_vertex: Vertex<NodeId>,
    change_id: NodeId,
    known: &mut K,
) -> Result<bool, TxnErr<T::GraphError>>
where
    K: FnMut(Hash) -> bool,
{
    for v in iter_alive_children(txn, channel, dest_vertex)? {
        let v = v?;
        debug!(
            "collect_unknown_children dest_vertex = {:?}, v = {:?}",
            dest_vertex, v
        );
        if v.introduced_by() == change_id || v.dest().change.is_root() {
            continue;
        }
        if v.introduced_by().is_root() {
            continue;
        }
        let mut not_del_by_change = true;
        for e in iter_adjacent(
            txn,
            channel,
            dest_vertex,
            EdgeFlags::PARENT | EdgeFlags::DELETED,
            EdgeFlags::all(),
        )? {
            let e = e?;
            if e.introduced_by() == v.introduced_by() {
                not_del_by_change = false;
                break;
            }
        }
        if not_del_by_change {
            let intro = txn.get_external(&v.introduced_by())?.unwrap().into();
            if !known(intro) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
