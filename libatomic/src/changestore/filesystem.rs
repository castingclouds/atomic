use super::*;
use crate::change::{Change, ChangeFile};
use crate::pristine::{Base32, Hash, Merkle, NodeId, Vertex};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// A file system change store.
pub struct FileSystem {
    change_cache: RefCell<lru_cache::LruCache<NodeId, ChangeFile>>,
    changes_dir: PathBuf,
}

impl Clone for FileSystem {
    fn clone(&self) -> Self {
        let len = self.change_cache.borrow().capacity();
        FileSystem {
            changes_dir: self.changes_dir.clone(),
            change_cache: RefCell::new(lru_cache::LruCache::new(len)),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
    #[error(transparent)]
    ChangeFile(#[from] crate::change::ChangeError),
    #[error(transparent)]
    Persist(#[from] tempfile::PersistError),
}

pub fn push_filename(changes_dir: &mut PathBuf, hash: &Hash) {
    let h32 = hash.to_base32();
    let (a, b) = h32.split_at(2);
    changes_dir.push(a);
    changes_dir.push(b);
    changes_dir.set_extension("change");
}

pub fn push_tag_filename(changes_dir: &mut PathBuf, hash: &Merkle) {
    let h32 = hash.to_base32();
    let (a, b) = h32.split_at(2);
    changes_dir.push(a);
    changes_dir.push(b);
    changes_dir.set_extension("tag");
}

pub fn pop_filename(changes_dir: &mut PathBuf) {
    changes_dir.pop();
    changes_dir.pop();
}

impl FileSystem {
    pub fn filename(&self, hash: &Hash) -> PathBuf {
        let mut path = self.changes_dir.clone();
        push_filename(&mut path, hash);
        debug!(
            "FileSystem::filename - changes_dir: {:?}, hash: {}, final path: {:?}",
            self.changes_dir,
            hash.to_base32(),
            path
        );
        path
    }

    pub fn tag_filename(&self, hash: &Merkle) -> PathBuf {
        let mut path = self.changes_dir.clone();
        push_tag_filename(&mut path, hash);
        debug!(
            "FileSystem::tag_filename - changes_dir: {:?}, hash: {}, final path: {:?}",
            self.changes_dir,
            hash.to_base32(),
            path
        );
        path
    }

    pub fn has_change(&self, hash: &Hash) -> bool {
        std::fs::metadata(&self.filename(hash)).is_ok()
    }

    /// Construct a `FileSystem`, starting from the root of the
    /// repository (i.e. the parent of the `.atomic` directory).
    pub fn from_root<P: AsRef<Path>>(root: P, cap: usize) -> Self {
        let dot_atomic = root.as_ref().join(crate::DOT_DIR);
        let changes_dir = dot_atomic.join("changes");
        Self::from_changes(changes_dir, cap)
    }

    /// Construct a `FileSystem`, starting from the root of the
    /// repository (i.e. the parent of the `.atomic` directory).
    pub fn from_changes(changes_dir: PathBuf, cap: usize) -> Self {
        std::fs::create_dir_all(&changes_dir).unwrap();
        FileSystem {
            changes_dir,
            change_cache: RefCell::new(lru_cache::LruCache::new(cap)),
        }
    }

    fn load<'a, F: Fn(NodeId) -> Option<Hash>>(
        &'a self,
        hash: F,
        change: NodeId,
    ) -> Result<
        std::cell::RefMut<'a, lru_cache::LruCache<NodeId, ChangeFile>>,
        crate::change::ChangeError,
    > {
        let mut change_cache = self.change_cache.borrow_mut();
        if !change_cache.contains_key(&change) {
            let h = hash(change).unwrap();
            let path = self.filename(&h);
            debug!("changefile: {:?}", path);
            let p = crate::change::ChangeFile::open(h, &path.to_str().unwrap())?;
            debug!("patch done");
            change_cache.insert(change, p);
        }
        Ok(change_cache)
    }

    pub fn save_from_buf(
        &self,
        buf: &[u8],
        hash: &Hash,
        change_id: Option<NodeId>,
    ) -> Result<(), crate::change::ChangeError> {
        Change::check_from_buffer(buf, hash)?;
        self.save_from_buf_unchecked(buf, hash, change_id)?;
        Ok(())
    }

    pub fn save_from_buf_unchecked(
        &self,
        buf: &[u8],
        hash: &Hash,
        change_id: Option<NodeId>,
    ) -> Result<(), std::io::Error> {
        let mut f = tempfile::NamedTempFile::new_in(&self.changes_dir)?;
        let file_name = self.filename(hash);
        use std::io::Write;
        f.write_all(buf)?;
        debug!("file_name = {:?}", file_name);
        std::fs::create_dir_all(file_name.parent().unwrap())?;
        f.persist(file_name)?;
        if let Some(ref change_id) = change_id {
            self.change_cache.borrow_mut().remove(change_id);
        }
        Ok(())
    }
}

impl ChangeStore for FileSystem {
    type Error = Error;
    fn has_contents(&self, hash: Hash, change_id: Option<NodeId>) -> bool {
        if let Some(ref change_id) = change_id {
            if let Some(l) = self.change_cache.borrow_mut().get_mut(change_id) {
                return l.has_contents();
            }
        }
        let path = self.filename(&hash);
        if let Ok(p) = crate::change::ChangeFile::open(hash, &path.to_str().unwrap()) {
            p.has_contents()
        } else {
            false
        }
    }

    fn get_header(&self, h: &Hash) -> Result<ChangeHeader, Self::Error> {
        let path = self.filename(h);
        let p = crate::change::ChangeFile::open(*h, &path.to_str().unwrap())?;
        Ok(p.hashed().header.clone())
    }

    fn get_tag_header(&self, h: &Merkle) -> Result<ChangeHeader, Self::Error> {
        let mut tag_path = self.changes_dir.clone();
        crate::changestore::filesystem::push_tag_filename(&mut tag_path, h);

        debug!(
            "get_tag_header: Looking for tag {} at path: {}",
            h.to_base32(),
            tag_path.display()
        );
        debug!(
            "get_tag_header: changes_dir = {}, tag file exists = {}",
            self.changes_dir.display(),
            tag_path.exists()
        );

        let mut tag_file = crate::tag::OpenTagFile::open(&tag_path, h).map_err(|e| {
            error!(
                "get_tag_header: Failed to open tag file at {}: {}",
                tag_path.display(),
                e
            );
            Self::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to open tag file at {}: {}", tag_path.display(), e),
            ))
        })?;

        tag_file.header().map_err(|e| {
            Self::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to read tag header: {}", e),
            ))
        })
    }

    fn get_contents<F: Fn(NodeId) -> Option<Hash>>(
        &self,
        hash: F,
        key: Vertex<NodeId>,
        buf: &mut [u8],
    ) -> Result<usize, Self::Error> {
        debug!("get_contents {:?}", key);
        if key.end <= key.start || key.is_root() {
            debug!("return 0");
            return Ok(0);
        }
        assert_eq!(key.end - key.start, buf.len());
        let mut cache = self.load(hash, key.change)?;
        let p = cache.get_mut(&key.change).unwrap();
        let n = p.read_contents(key.start.into(), buf)?;
        debug!("get_contents {:?}", n);
        Ok(n)
    }
    fn get_contents_ext(
        &self,
        key: Vertex<Option<Hash>>,
        buf: &mut [u8],
    ) -> Result<usize, Self::Error> {
        if let Some(change) = key.change {
            assert_eq!(key.end.us() - key.start.us(), buf.len());
            if key.end <= key.start {
                return Ok(0);
            }
            let path = self.filename(&change);
            let mut p = crate::change::ChangeFile::open(change, &path.to_str().unwrap())?;
            let n = p.read_contents(key.start.into(), buf)?;
            Ok(n)
        } else {
            Ok(0)
        }
    }
    fn change_deletes_position<F: Fn(NodeId) -> Option<Hash>>(
        &self,
        hash: F,
        change: NodeId,
        pos: Position<Option<Hash>>,
    ) -> Result<Vec<Hash>, Self::Error> {
        let mut cache = self.load(hash, change)?;
        let p = cache.get_mut(&change).unwrap();
        let mut v = Vec::new();
        for c in p.hashed().changes.iter() {
            for c in c.iter() {
                v.extend(c.deletes_pos(pos).into_iter())
            }
        }
        Ok(v)
    }
    fn save_change<
        E: From<Self::Error> + From<ChangeError>,
        F: FnOnce(&mut Change, &Hash) -> Result<(), E>,
    >(
        &self,
        p: &mut Change,
        ff: F,
    ) -> Result<Hash, E> {
        let mut f = match tempfile::NamedTempFile::new_in(&self.changes_dir) {
            Ok(f) => f,
            Err(e) => return Err(E::from(Error::from(e))),
        };
        let hash = {
            let w = std::io::BufWriter::new(&mut f);
            p.serialize(w, ff)?
        };
        let file_name = self.filename(&hash);
        if let Err(e) = std::fs::create_dir_all(file_name.parent().unwrap()) {
            return Err(E::from(Error::from(e)));
        }
        debug!("file_name = {:?}", file_name);
        if let Err(e) = f.persist(file_name) {
            return Err(E::from(Error::from(e)));
        }
        Ok(hash)
    }
    fn del_change(&self, hash: &Hash) -> Result<bool, Self::Error> {
        let file_name = self.filename(hash);
        debug!("file_name = {:?}", file_name);
        let result = std::fs::remove_file(&file_name).is_ok();
        std::fs::remove_dir(file_name.parent().unwrap()).unwrap_or(()); // fails silently if there are still changes with the same 2-letter prefix.
        Ok(result)
    }
    fn get_change(&self, h: &Hash) -> Result<Change, Self::Error> {
        let file_name = self.filename(h);
        let file_name = file_name.to_str().unwrap();
        debug!("get_change: looking for hash {}", h.to_base32());
        debug!("get_change: trying change file at {:?}", file_name);

        // First try to load as a regular change file
        match Change::deserialize(&file_name, Some(h)) {
            Ok(change) => {
                debug!("get_change: found regular change file");
                Ok(change)
            }
            Err(change_err) => {
                debug!(
                    "get_change: change file not found or couldn't deserialize: {:?}",
                    change_err
                );

                // If the regular change file doesn't exist, try loading as a tag file
                // Tags use their merkle hash as the filename, and Hash IS Merkle now
                let tag_path = self.tag_filename(h);
                debug!("get_change: checking for tag file at {:?}", tag_path);
                debug!("get_change: self.changes_dir = {:?}", self.changes_dir);

                // Let's check what files actually exist in the directory
                let hash_str = h.to_base32();
                if hash_str.len() >= 2 {
                    let (prefix, _) = hash_str.split_at(2);
                    let dir_path = self.changes_dir.join(prefix);
                    debug!("get_change: checking directory {:?}", dir_path);
                    if dir_path.exists() {
                        debug!("get_change: directory exists, listing contents:");
                        if let Ok(entries) = std::fs::read_dir(&dir_path) {
                            for entry in entries {
                                if let Ok(e) = entry {
                                    debug!("  - {:?}", e.file_name());
                                }
                            }
                        }
                    } else {
                        debug!("get_change: directory does NOT exist: {:?}", dir_path);
                    }
                }

                if tag_path.exists() {
                    debug!(
                        "get_change: tag file EXISTS at {:?}, creating synthetic change",
                        tag_path
                    );

                    // Create a synthetic change that represents the tag
                    // This allows the rest of the system to treat tags as dependencies
                    use crate::change::{Author, ChangeHeader_, Hashed};
                    use crate::tag::OpenTagFile;

                    // Open and read the tag file
                    let header = match OpenTagFile::open(&tag_path, h) {
                        Ok(mut tag_file) => match tag_file.header() {
                            Ok(header) => header,
                            Err(tag_err) => {
                                debug!("get_change: failed to read tag header: {}", tag_err);
                                // Return the original change error since we couldn't read the tag either
                                return Err(Error::ChangeFile(change_err));
                            }
                        },
                        Err(tag_err) => {
                            debug!("get_change: failed to open tag file: {}", tag_err);
                            // Return the original change error since we couldn't open the tag either
                            return Err(Error::ChangeFile(change_err));
                        }
                    };

                    // Create a change that represents this tag
                    // Use the first author if available, or create a default one
                    let author = if let Some(first_author) = header.authors.first() {
                        first_author.clone()
                    } else {
                        let mut author_map = std::collections::BTreeMap::new();
                        author_map.insert("name".to_string(), "Unknown".to_string());
                        Author(author_map)
                    };

                    let hashed = Hashed {
                        version: 1,
                        header: ChangeHeader_ {
                            message: header.message.clone(),
                            description: header.description.clone(),
                            timestamp: header.timestamp,
                            authors: vec![author],
                        },
                        dependencies: Vec::new(), // Tags consolidate dependencies
                        extra_known: Vec::new(),
                        metadata: Vec::new(),
                        changes: Vec::new(), // No hunks in a tag
                        contents_hash: *h,   // Use the tag's hash
                        tag: Some(crate::change::TagMetadata {
                            version: None,
                            channel: String::new(),
                            consolidated_change_count: 0,
                            dependency_count_before: 0,
                            consolidated_changes: Vec::new(),
                            previous_consolidation: None,
                            consolidates_since: None,
                            created_by: None,
                            metadata: std::collections::HashMap::new(),
                        }),
                    };

                    debug!("get_change: successfully created synthetic change from tag");
                    Ok(crate::change::Change {
                        offsets: crate::change::Offsets::default(),
                        hashed,
                        unhashed: None,
                        contents: Vec::new(),
                    })
                } else {
                    // Neither change file nor tag file exists
                    debug!(
                        "get_change: neither change file nor tag file found for {}",
                        h.to_base32()
                    );
                    debug!(
                        "get_change: change path: {:?} exists: {}",
                        file_name,
                        std::path::Path::new(file_name).exists()
                    );
                    debug!(
                        "get_change: tag path: {:?} exists: {}",
                        tag_path,
                        tag_path.exists()
                    );

                    // Let's also check if we're looking in the right place
                    debug!(
                        "get_change: current working directory: {:?}",
                        std::env::current_dir()
                    );
                    debug!(
                        "get_change: ATOMIC_ROOT env var: {:?}",
                        std::env::var("ATOMIC_ROOT")
                    );

                    // Return the original error from trying to load the change file
                    Err(Error::ChangeFile(change_err))
                }
            }
        }
    }
}
