use crate::pristine::sanakirja::{MutTxn, SanakirjaError, P, UP};
use crate::pristine::*;
use crate::HashSet;
use crate::TxnT;
use log::*;
use serde_derive::*;
use std::io::Read;
use std::io::{Seek, SeekFrom};
use std::path::Path;

pub mod txn;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FileHeader {
    pub version: u64,
    pub header: u64,
    pub channel: u64,
    pub unhashed: u64,
    pub total: u64,
    pub offsets: DbOffsets,
    pub state: Merkle,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DbOffsets {
    pub internal: u64,
    pub external: u64,
    pub graph: u64,
    pub changes: u64,
    pub revchanges: u64,
    pub states: u64,
    pub tags: u64,
    pub apply_counter: u64,
    pub size: u64,
}

pub struct OpenTagFile {
    pub header: FileHeader,
    pub file: std::fs::File,
}

#[derive(Debug, Error)]
pub enum TagError {
    #[error("Version mismatch")]
    VersionMismatch,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Bincode(#[from] bincode::Error),
    #[error("Tag file is corrupt")]
    BincodeDe(bincode::Error),
    #[error(transparent)]
    Zstd(#[from] zstd_seekable::Error),
    #[error(transparent)]
    Txn(SanakirjaError),
    #[error("Synchronisation error")]
    Sync,
    #[error("Wrong state, expected {}, got {}", expected.to_base32(), got.to_base32())]
    WrongHash { expected: Merkle, got: Merkle },
}

impl From<TxnErr<SanakirjaError>> for TagError {
    fn from(e: TxnErr<SanakirjaError>) -> Self {
        TagError::Txn(e.0)
    }
}

impl From<TxnErr<SanakirjaError>> for TxnErr<TagError> {
    fn from(e: TxnErr<SanakirjaError>) -> Self {
        TxnErr(TagError::Txn(e.0))
    }
}

impl OpenTagFile {
    pub fn open<P: AsRef<Path>>(p: P, expected: &Merkle) -> Result<Self, TagError> {
        let mut file = std::fs::File::open(p)?;
        let mut off = [0u8; std::mem::size_of::<FileHeader>() as usize];
        file.read_exact(&mut off)?;
        let header: FileHeader = bincode::deserialize(&off).map_err(TagError::BincodeDe)?;
        if &header.state == expected {
            Ok(OpenTagFile { header, file })
        } else {
            Err(TagError::WrongHash {
                expected: *expected,
                got: header.state,
            })
        }
    }

    pub fn header(&mut self) -> Result<crate::change::ChangeHeader, TagError> {
        self.file.seek(SeekFrom::Start(self.header.header))?;
        Ok(bincode::deserialize_from(&mut self.file).map_err(TagError::BincodeDe)?)
    }

    pub fn state(&self) -> Merkle {
        self.header.state.clone()
    }

    pub fn short<W: std::io::Write>(&mut self, mut w: W) -> Result<(), TagError> {
        let mut header_buf = vec![0u8; (self.header.channel - self.header.header) as usize];

        self.file.seek(SeekFrom::Start(self.header.header))?;
        self.file.read_exact(&mut header_buf)?;
        debug!("header_buf = {:?}", header_buf);
        let mut off = FileHeader {
            version: VERSION,
            header: 0,
            channel: 0,
            unhashed: 0,
            total: 0,
            offsets: DbOffsets::default(),
            state: self.header.state.clone(),
        };
        off.header = bincode::serialized_size(&off)?;
        off.channel = off.header + header_buf.len() as u64;
        off.total = off.channel;
        let mut off_buf = Vec::with_capacity(off.header as usize);
        bincode::serialize_into(&mut off_buf, &off)?;
        w.write_all(&off_buf)?;
        w.write_all(&header_buf)?;
        Ok(())
    }
}

pub fn read_short<R: std::io::Read + std::io::Seek>(
    mut file: R,
    expected: &Merkle,
) -> Result<crate::change::ChangeHeader, TagError> {
    let mut off = [0u8; std::mem::size_of::<FileHeader>() as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut off)?;
    let header: FileHeader = bincode::deserialize(&off).map_err(TagError::BincodeDe)?;
    debug!("header = {:?}", header);
    if &header.state == expected {
        file.seek(SeekFrom::Start(header.header))?;
        Ok(bincode::deserialize_from(file).map_err(TagError::BincodeDe)?)
    } else {
        Err(TagError::WrongHash {
            expected: *expected,
            got: header.state,
        })
    }
}

pub const VERSION: u64 = 7;
pub const VERSION_NOENC: u64 = 5;

pub(crate) const BLOCK_SIZE: usize = 4096;

pub fn restore_channel(
    mut tag: OpenTagFile,
    _txn: &mut MutTxn<()>,
    _name: &str,
) -> Result<ChannelRef<MutTxn<()>>, TagError> {
    tag.file.seek(SeekFrom::Start(tag.header.channel))?;
    let mut comp = vec![0; (tag.header.unhashed - tag.header.channel) as usize];
    debug!("tag header {:?}", tag.header);
    tag.file.read_exact(&mut comp)?;
    debug!("{:?} {:?}", &comp[..20], comp.len());
    debug!("{:?}", &comp[comp.len() - 20..]);
    let mut buf = vec![0; tag.header.offsets.size as usize];
    zstd_seekable::Seekable::init_buf(&comp)?
        .decompress(&mut buf, 0)
        .unwrap();
    let filetxn = Txn::from_slice(&mut buf);
    let external: ::sanakirja::btree::Db_<NodeId, SerializedHash, UP<NodeId, SerializedHash>> =
        unsafe { ::sanakirja::btree::Db_::from_page(tag.header.offsets.external) };
    let mut vi = Vec::new();
    for i in ::sanakirja::btree::iter(&filetxn, &external, None).unwrap() {
        debug!("{:?}", i);
        vi.push(i.unwrap());
    }
    vi.sort();
    // BREAKING CHANGE: Tag format has changed
    // Old tag files stored Pair<SerializedMerkle, SerializedMerkle>
    // New format stores TagBytes (full serialized Tag metadata)
    // Users must regenerate tags with `atomic tag create`
    //
    // The following restore operations are commented out as they are incompatible
    // with the new tag format:
    /*
    debug!("restoring graph {:?}", vi);
    let graph = restore::<_, _, _, _, sanakirja::btree::page::Page<_, _>>(
        &filetxn,
        txn,
        tag.header.offsets.graph,
        |file_txn, txn, k: &Vertex<NodeId>, v: &SerializedEdge| {
            // ... restore logic ...
        },
    )?;

    debug!("restoring changes");
    let changes = restore::<_, _, _, _, sanakirja::btree::page::Page<_, _>>(
        &filetxn,
        txn,
        tag.header.offsets.changes,
        |file_txn, txn, k: &NodeId, v: &L64| {
            // ... restore logic ...
        },
    )?;

    debug!("restoring revchanges");
    let revchanges = restore::<_, _, _, _, sanakirja::btree::page::Page<_, _>>(
        &filetxn,
        txn,
        tag.header.offsets.revchanges,
        |file_txn, txn, k: &L64, v: &Pair<NodeId, SerializedMerkle>| {
            // ... restore logic ...
        },
    )?;

    debug!("restoring states");
    let states = restore::<_, _, _, _, sanakirja::btree::page::Page<_, _>>(
        &filetxn,
        txn,
        tag.header.offsets.states,
        |_, _, k: &SerializedMerkle, v: &L64| Ok((k.clone(), *v)),
    )?;
    */

    return Err(TagError::VersionMismatch.into());
}

struct Txn<'a> {
    data: *mut u8,
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Txn<'a> {
    fn from_slice(s: &'a mut [u8]) -> Self {
        Txn {
            data: s.as_mut_ptr(),
            marker: std::marker::PhantomData,
        }
    }
}

impl<'a> ::sanakirja::LoadPage for Txn<'a> {
    type Error = ::sanakirja::CRCError;
    unsafe fn load_page(&self, off: u64) -> Result<::sanakirja::CowPage, ::sanakirja::CRCError> {
        Ok(::sanakirja::CowPage {
            data: unsafe { self.data.add(off as usize) },
            offset: off,
        })
    }
}

#[allow(dead_code)]
fn restore<
    K: ::sanakirja::UnsizedStorable + PartialEq,
    V: ::sanakirja::UnsizedStorable + PartialEq,
    P: ::sanakirja::btree::BTreeMutPage<K, V>,
    F,
>(
    file_txn: &Txn,
    txn: &mut crate::pristine::sanakirja::MutTxn<()>,
    pending: u64,
    f: F,
) -> Result<::sanakirja::btree::Db_<K, V, P>, TxnErr<SanakirjaError>>
where
    F: Fn(
        &Txn,
        &mut crate::pristine::sanakirja::MutTxn<()>,
        &K,
        &V,
    ) -> Result<(K, V), TxnErr<SanakirjaError>>,
{
    use ::sanakirja::AllocPage;
    let mut dict = HashSet::default();
    let page = unsafe { txn.txn.alloc_page()? };
    let result = page.0.offset;
    let mut pending = vec![(pending, page)];
    while let Some((offset, mut new_page_)) = pending.pop() {
        debug!("{:?}", offset);
        let page = ::sanakirja::CowPage {
            data: unsafe { file_txn.data.offset(offset as isize) },
            offset,
        };
        let mut curs = P::cursor_first(&page);
        let mut new_curs = P::cursor_first(&new_page_.0);
        P::init(&mut new_page_);
        unsafe {
            let left = P::left_child(page.as_page(), &curs);
            if left != 0 {
                assert!(dict.insert(left));
                let new_page = txn.txn.alloc_page()?;
                let off = new_page.0.offset;
                P::set_left_child(&mut new_page_, &new_curs, off);
                pending.push((left, new_page));
            }
        }
        while let Some((k_, v_, r)) = P::next(&txn.txn, page.as_page(), &mut curs) {
            let (k, v) = f(file_txn, txn, k_, v_)?;
            assert_eq!(&k, k_);
            assert_eq!(&v, v_);
            let r = if r > 0 {
                assert!(dict.insert(r));
                let new_page = unsafe { txn.txn.alloc_page()? };
                let off = new_page.0.offset;
                pending.push((r, new_page));
                off
            } else {
                0
            };
            unsafe { P::put_mut(&mut txn.txn, &mut new_page_, &mut new_curs, &k, &v, r) }
            P::move_next(&mut new_curs);
        }
    }
    Ok(unsafe { ::sanakirja::btree::Db_::from_page(result) })
}

pub fn from_channel<
    W: std::io::Write,
    T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage,
>(
    txn: &crate::pristine::sanakirja::GenericTxn<T>,
    channel: &str,
    header: &crate::change::ChangeHeader,
    mut w: W,
) -> Result<Merkle, TagError> {
    let out = Vec::with_capacity(1 << 16);
    let (out, offsets, state) = compress_channel(txn, channel, out)?;
    debug!("{:?} {:?}", &out[..20], out.len());
    debug!("{:?}", &out[out.len() - 20..]);
    let mut header_buf = Vec::with_capacity(1 << 10);
    bincode::serialize_into(&mut header_buf, header).unwrap();

    let mut off = FileHeader {
        version: VERSION,
        header: 0,
        channel: 0,
        unhashed: 0,
        total: 0,
        offsets,
        state: state.clone(),
    };
    off.header = bincode::serialized_size(&off)?;
    off.channel = off.header + header_buf.len() as u64;
    off.unhashed = off.channel + out.len() as u64;
    off.total = off.unhashed;

    let mut off_buf = Vec::with_capacity(off.header as usize);
    bincode::serialize_into(&mut off_buf, &off)?;
    debug!("off_buf = {:?}", off_buf.len());
    w.write_all(&off_buf)?;
    debug!("header_buf = {:?}", header_buf.len());
    w.write_all(&header_buf)?;
    debug!("out = {:?}", out.len());
    w.write_all(&out)?;
    Ok(state)
}

const LEVEL: usize = 10;
const PIPE_LEN: usize = 10;

fn compress_channel<
    W: std::io::Write + Send + 'static,
    T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage,
>(
    txn: &crate::pristine::sanakirja::GenericTxn<T>,
    channel: &str,
    mut to: W,
) -> Result<(W, DbOffsets, Merkle), TagError> {
    debug!("int = {:?}", txn.internal.db);
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(PIPE_LEN);
    let (bsender, breceiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(PIPE_LEN);
    for _ in 0..PIPE_LEN {
        bsender.send(vec![0; 4096]).map_err(|_| TagError::Sync)?;
    }

    let t = std::thread::spawn(move || -> Result<(W, usize), TagError> {
        let mut comp = zstd_seekable::SeekableCStream::new(LEVEL, BLOCK_SIZE).unwrap();
        let mut out = [0; BLOCK_SIZE];
        let mut n = 0;
        while let Ok(input) = receiver.recv() {
            n += BLOCK_SIZE;
            let mut input_off = 0;
            let mut output_off = 0;
            while input_off < BLOCK_SIZE as usize {
                let (a, b) = comp
                    .compress(&mut out[output_off..], &input[input_off..])
                    .unwrap();
                output_off += a;
                input_off += b;
            }
            to.write_all(&out[..output_off]).unwrap();
            bsender.send(input).map_err(|_| TagError::Sync)?;
        }
        while let Ok(n) = comp.end_stream(&mut out) {
            if n == 0 {
                break;
            }
            to.write_all(&out[..n])?;
        }
        Ok((to, n))
    });
    let channel = txn.load_channel(channel)?.unwrap();
    let channel = channel.read();
    let mut new = 0;
    debug!("copying internal");
    let internal = copy::<SerializedHash, NodeId, UP<SerializedHash, NodeId>, _>(
        txn,
        txn.internal.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying external");
    let external = copy::<NodeId, SerializedHash, UP<NodeId, SerializedHash>, _>(
        txn,
        txn.external.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying graph");
    let graph = copy::<Vertex<NodeId>, SerializedEdge, P<Vertex<NodeId>, SerializedEdge>, _>(
        txn,
        channel.graph.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying changes");
    let changes = copy::<NodeId, L64, P<NodeId, L64>, _>(
        txn,
        channel.changes.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying revchanges");
    let revchanges = copy::<
        L64,
        Pair<NodeId, SerializedMerkle>,
        UP<L64, Pair<NodeId, SerializedMerkle>>,
        _,
    >(
        &txn,
        channel.revchanges.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying states");
    let states = copy::<SerializedMerkle, L64, UP<SerializedMerkle, L64>, _>(
        txn,
        channel.states.db.into(),
        &mut new,
        &sender,
        &breceiver,
    )?;
    debug!("copying tags");
    let tags = copy::<
        L64,
        Pair<SerializedMerkle, SerializedMerkle>,
        P<L64, Pair<SerializedMerkle, SerializedMerkle>>,
        _,
    >(&txn, channel.tags.db.into(), &mut new, &sender, &breceiver)?;
    std::mem::drop(sender);
    let (w, n) = t.join().unwrap()?;
    let state = crate::pristine::current_state(txn, &channel)?;
    Ok((
        w,
        DbOffsets {
            internal,
            external,
            graph,
            changes,
            revchanges,
            states,
            tags: tags,
            apply_counter: channel.apply_counter,
            size: n as u64,
        },
        state,
    ))
}

struct CopyTxn {}

impl ::sanakirja::AllocPage for CopyTxn {
    /// Allocate a single page.
    unsafe fn alloc_page(&mut self) -> Result<::sanakirja::MutPage, Self::Error> {
        unimplemented!()
    }

    /// Allocate a single page.
    unsafe fn alloc_page_no_dirty(&mut self) -> Result<::sanakirja::MutPage, Self::Error> {
        unimplemented!()
    }

    /// Allocate many contiguous pages, return the first one
    unsafe fn alloc_contiguous(&mut self, _: u64) -> Result<::sanakirja::MutPage, Self::Error> {
        unimplemented!()
    }

    /// Increment the reference count for page `off`.
    fn incr_rc(&mut self, _: u64) -> Result<usize, Self::Error> {
        unimplemented!()
    }

    unsafe fn decr_rc(&mut self, _: u64) -> Result<usize, Self::Error> {
        unimplemented!()
    }

    unsafe fn decr_rc_owned(&mut self, _: u64) -> Result<usize, Self::Error> {
        unimplemented!()
    }
}

impl ::sanakirja::LoadPage for CopyTxn {
    type Error = std::convert::Infallible;
    unsafe fn load_page(&self, _: u64) -> Result<::sanakirja::CowPage, Self::Error> {
        unimplemented!()
    }
}

fn copy<
    K: ::sanakirja::UnsizedStorable,
    V: ::sanakirja::UnsizedStorable,
    P: ::sanakirja::btree::BTreeMutPage<K, V>,
    T: ::sanakirja::LoadPage<Error = ::sanakirja::Error> + ::sanakirja::RootPage,
>(
    txn: &crate::pristine::sanakirja::GenericTxn<T>,
    pending_: u64,
    new_page: &mut u64,
    sender: &std::sync::mpsc::SyncSender<Vec<u8>>,
    buffers: &std::sync::mpsc::Receiver<Vec<u8>>,
) -> Result<u64, TagError> {
    let mut dict = HashSet::default();
    let result = *new_page;
    let mut pending = std::collections::VecDeque::new();
    pending.push_back((pending_, *new_page));
    *new_page += BLOCK_SIZE as u64;
    while let Some((old_page_off, new_page_)) = pending.pop_front() {
        let page = unsafe { txn.txn.load_page(old_page_off).unwrap() };
        let mut memory = buffers.recv().map_err(|_| TagError::Sync)?;
        let mut new_page_ = ::sanakirja::MutPage(::sanakirja::CowPage {
            data: memory.as_mut_ptr(),
            offset: new_page_,
        });
        P::init(&mut new_page_);
        let mut curs = P::cursor_first(&page);
        let mut new_curs = P::cursor_first(&new_page_.0);
        unsafe {
            let left = P::left_child(page.as_page(), &curs);
            if left != 0 {
                let new = *new_page;
                *new_page += BLOCK_SIZE as u64;
                P::set_left_child(&mut new_page_, &new_curs, new);
                pending.push_back((left, new));
            }
        }
        while let Some((k, v, r)) = P::next(&txn.txn, page.as_page(), &mut curs) {
            let r = if r > 0 {
                assert!(dict.insert(r));
                let new = *new_page;
                *new_page += BLOCK_SIZE as u64;
                pending.push_back((r, new));
                unsafe {
                    P::set_left_child(&mut new_page_, &curs, new);
                }
                new
            } else {
                0
            };
            debug!("put {:?} {:?}", k, v);
            unsafe { P::put_mut(&mut CopyTxn {}, &mut new_page_, &mut new_curs, k, v, r) }
            P::move_next(&mut new_curs);
        }
        sender.send(memory).unwrap();
    }
    Ok(result)
}
