use crate::{changestore::ChangeStore, pristine::*};

pub const START_MARKER: &str = ">>>>>>>";

pub const SEPARATOR: &str = "=======";

pub const END_MARKER: &str = "<<<<<<<";

/// A trait for outputting keys and their contents. This trait allows
/// to retain more information about conflicts than directly
/// outputting as bytes to a `Write`. The diff algorithm uses that
/// information, for example.
pub trait VertexBuffer {
    fn output_line<E, F>(&mut self, key: Vertex<NodeId>, contents: F) -> Result<(), E>
    where
        E: From<std::io::Error>,
        F: FnOnce(&mut [u8]) -> Result<(), E>;

    fn output_conflict_marker<C: ChangeStore>(
        &mut self,
        s: &str,
        id: usize,
        sides: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error>;
    fn begin_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        side: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.output_conflict_marker(START_MARKER, id, side)
    }
    fn begin_zombie_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        add_del: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.output_conflict_marker(START_MARKER, id, add_del)
    }
    fn begin_cyclic_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.output_conflict_marker::<C>(START_MARKER, id, None)
    }
    fn conflict_next<C: ChangeStore>(
        &mut self,
        id: usize,
        side: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.output_conflict_marker(SEPARATOR, id, side)
    }
    fn end_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.output_conflict_marker::<C>(END_MARKER, id, None)
    }
    fn end_zombie_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.end_conflict::<C>(id)
    }
    fn end_cyclic_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.output_conflict_marker::<C>(END_MARKER, id, None)
    }
}

pub(crate) struct ConflictsWriter<'a, 'b, W: std::io::Write> {
    pub w: W,
    pub lines: usize,
    pub new_line: bool,
    pub path: &'b str,
    pub inode_vertex: Position<NodeId>,
    pub conflicts: &'a mut Vec<crate::output::Conflict>,
    pub buf: Vec<u8>,
}

impl<'a, 'b, W: std::io::Write> ConflictsWriter<'a, 'b, W> {
    pub fn new(
        w: W,
        path: &'b str,
        inode_vertex: Position<NodeId>,
        conflicts: &'a mut Vec<crate::output::Conflict>,
    ) -> Self {
        ConflictsWriter {
            inode_vertex,
            w,
            new_line: true,
            lines: 1,
            path,
            conflicts,
            buf: Vec::new(),
        }
    }
}

impl<'a, 'b, W: std::io::Write> std::ops::Deref for ConflictsWriter<'a, 'b, W> {
    type Target = W;
    fn deref(&self) -> &Self::Target {
        &self.w
    }
}

impl<'a, 'b, W: std::io::Write> std::ops::DerefMut for ConflictsWriter<'a, 'b, W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.w
    }
}

impl<'a, 'b, W: std::io::Write> VertexBuffer for ConflictsWriter<'a, 'b, W> {
    fn output_line<E, C>(&mut self, v: Vertex<NodeId>, c: C) -> Result<(), E>
    where
        E: From<std::io::Error>,
        C: FnOnce(&mut [u8]) -> Result<(), E>,
    {
        self.buf.resize(v.end - v.start, 0);
        c(&mut self.buf)?;
        debug!("vbuf {:?} {:?}", v, std::str::from_utf8(&self.buf));
        let ends_with_newline = self.buf.ends_with(b"\n");
        self.lines += self.buf.iter().filter(|c| **c == b'\n').count();
        self.w.write_all(&self.buf)?;
        if !self.buf.is_empty() {
            // empty "lines" (such as in the beginning of a file)
            // don't change the status of self.new_line.
            self.new_line = ends_with_newline;
        }
        Ok(())
    }

    fn output_conflict_marker<C: ChangeStore>(
        &mut self,
        s: &str,
        id: usize,
        sides: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        debug!("output_conflict_marker {:?}", self.new_line);
        if !self.new_line {
            self.lines += 2;
            self.w.write_all(b"\n")?;
        } else {
            self.lines += 1;
            debug!("{:?}", s.as_bytes());
        }
        write!(self.w, "{} {}", s, id)?;
        match sides {
            Some((changes, sides)) => {
                for side in sides {
                    let h = side.to_base32();
                    write!(
                        self.w,
                        " [{} {}]",
                        h.split_at(8).0,
                        change_message(changes, side)
                    )?;
                }
            }
            None => (),
        };
        self.w.write_all(b"\n")?;
        self.new_line = true;
        Ok(())
    }

    fn begin_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        sides: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.conflicts.push(crate::output::Conflict::Order {
            path: self.path.to_string(),
            inode_vertex: [self.inode_vertex],
            line: self.lines,
            changes: sides
                .iter()
                .flat_map(|(_, b)| b.iter())
                .cloned()
                .cloned()
                .collect(),
            id,
        });
        self.output_conflict_marker(START_MARKER, id, sides)
    }
    fn begin_zombie_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        add_del: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.conflicts.push(crate::output::Conflict::Zombie {
            path: self.path.to_string(),
            inode_vertex: [self.inode_vertex],
            line: self.lines,
            changes: add_del
                .iter()
                .flat_map(|(_, b)| b.iter())
                .cloned()
                .cloned()
                .collect(),
            id,
        });
        self.output_conflict_marker(START_MARKER, id, add_del)
    }
    fn begin_cyclic_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.conflicts.push(crate::output::Conflict::Cyclic {
            path: self.path.to_string(),
            inode_vertex: [self.inode_vertex],
            line: self.lines,
            changes: Vec::new(),
            id,
        });
        self.output_conflict_marker::<C>(START_MARKER, id, None)
    }
    fn conflict_next<C: ChangeStore>(
        &mut self,
        id_: usize,
        sides: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        for conflict in self.conflicts.iter_mut().rev() {
            match conflict {
                crate::output::Conflict::Order { id, changes, .. } if *id == id_ => {
                    changes.extend(sides.into_iter().flat_map(|(_, b)| b.into_iter()).cloned())
                }
                crate::output::Conflict::Zombie { id, changes, .. } if *id == id_ => {
                    changes.extend(sides.into_iter().flat_map(|(_, b)| b.into_iter()).cloned())
                }
                crate::output::Conflict::Cyclic { id, changes, .. } if *id == id_ => {
                    changes.extend(sides.into_iter().flat_map(|(_, b)| b.into_iter()).cloned())
                }
                _ => break,
            }
        }
        self.output_conflict_marker(SEPARATOR, id_, sides)
    }
}

pub fn change_message<C: ChangeStore>(changes: &C, hash: &Hash) -> String {
    match changes.get_header(hash) {
        Ok(header) => {
            if let Some(l) = header.message.lines().next() {
                l.to_string()
            } else {
                String::new()
            }
        }
        Err(_e) => "".to_string(),
    }
}

pub struct Writer<W: std::io::Write> {
    w: W,
    buf: Vec<u8>,
    new_line: bool,
    is_zombie: bool,
}

impl<W: std::io::Write> Writer<W> {
    pub fn new(w: W) -> Self {
        Writer {
            w,
            new_line: true,
            buf: Vec::new(),
            is_zombie: false,
        }
    }
    pub fn into_inner(self) -> W {
        self.w
    }
}

impl<W: std::io::Write> std::ops::Deref for Writer<W> {
    type Target = W;
    fn deref(&self) -> &Self::Target {
        &self.w
    }
}

impl<W: std::io::Write> std::ops::DerefMut for Writer<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.w
    }
}

impl<W: std::io::Write> VertexBuffer for Writer<W> {
    fn output_line<E, C>(&mut self, v: Vertex<NodeId>, c: C) -> Result<(), E>
    where
        E: From<std::io::Error>,
        C: FnOnce(&mut [u8]) -> Result<(), E>,
    {
        self.buf.resize(v.end - v.start, 0);
        c(&mut self.buf)?;
        debug!("vbuf {:?} {:?}", v, std::str::from_utf8(&self.buf));
        let ends_with_newline = self.buf.ends_with(b"\n");
        self.w.write_all(&self.buf[..])?;
        if !self.buf.is_empty() {
            // empty "lines" (such as in the beginning of a file)
            // don't change the status of self.new_line.
            self.new_line = ends_with_newline;
        }
        Ok(())
    }

    fn output_conflict_marker<C: ChangeStore>(
        &mut self,
        s: &str,
        id: usize,
        sides: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        debug!("output_conflict_marker {:?}", self.new_line);
        if !self.new_line {
            self.w.write_all(b"\n")?;
        }
        write!(self.w, "{} {}", s, id)?;
        match sides {
            Some((changes, sides)) => {
                for side in sides {
                    let h = side.to_base32();
                    write!(
                        self.w,
                        " [{} {}]",
                        h.split_at(8).0,
                        change_message(changes, side)
                    )?;
                }
            }
            None => (),
        };
        self.w.write_all(b"\n")?;
        Ok(())
    }

    fn begin_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        side: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        self.output_conflict_marker(START_MARKER, id, side)
    }
    fn end_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.is_zombie = false;
        self.output_conflict_marker::<C>(END_MARKER, id, None)
    }
    fn begin_zombie_conflict<C: ChangeStore>(
        &mut self,
        id: usize,
        add_del: Option<(&C, &[&Hash])>,
    ) -> Result<(), std::io::Error> {
        if self.is_zombie {
            Ok(())
        } else {
            self.is_zombie = true;
            self.output_conflict_marker(START_MARKER, id, add_del)
        }
    }
    fn end_zombie_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.is_zombie = false;
        self.output_conflict_marker::<C>(END_MARKER, id, None)
    }
    fn begin_cyclic_conflict<C: ChangeStore>(&mut self, id: usize) -> Result<(), std::io::Error> {
        self.output_conflict_marker::<C>(START_MARKER, id, None)
    }
}
