use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use lazy_static::lazy_static;
use libatomic::pristine::{
    sanakirja::MutTxn, Base32, ChannelRef, GraphIter, Hash, Merkle, MutTxnT, NodeId, NodeType,
    RemoteRef, SerializedMerkle, TxnT,
};
use libatomic::DOT_DIR;
use libatomic::{ChannelTxnT, DepsTxnT, GraphTxnT, MutTxnTExt, TxnTExt};
use log::{debug, info};

use atomic_config::*;
use atomic_identity::Complete;
use atomic_repository::*;

pub mod ssh;
use ssh::*;

pub mod local;
use local::*;

pub mod http;
use http::*;

pub mod attribution;

use atomic_interaction::{
    ProgressBar, Spinner, APPLY_MESSAGE, COMPLETE_MESSAGE, DOWNLOAD_MESSAGE, UPLOAD_MESSAGE,
};

pub const PROTOCOL_VERSION: usize = 4;

pub enum RemoteRepo {
    Local(Local),
    Ssh(Ssh),
    Http(Http),
    LocalChannel(String),
    None,
}

/// Node-type-aware structure representing any node in the DAG
///
/// Following AGENTS.md principles: "Changes and tags are just different types
/// of nodes in the same Directed Acyclic Graph (DAG)"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Node {
    /// The hash identifying this node
    pub hash: Hash,
    /// The type of this node (Change or Tag)
    pub node_type: NodeType,
    /// The channel state after applying this node
    pub state: Merkle,
}

impl Node {
    /// Create a new change node
    pub fn change(hash: Hash, state: Merkle) -> Self {
        Self {
            hash,
            node_type: NodeType::Change,
            state,
        }
    }

    /// Create a new tag node
    pub fn tag(hash: Hash, state: Merkle) -> Self {
        Self {
            hash,
            node_type: NodeType::Tag,
            state,
        }
    }

    /// Check if this node is a change
    pub fn is_change(&self) -> bool {
        self.node_type == NodeType::Change
    }

    /// Check if this node is a tag
    pub fn is_tag(&self) -> bool {
        self.node_type == NodeType::Tag
    }

    /// Get the node type as a string marker for protocol serialization
    pub fn type_marker(&self) -> &'static str {
        match self.node_type {
            NodeType::Change => "C",
            NodeType::Tag => "T",
        }
    }

    /// Create a Node from a type marker character
    pub fn from_type_marker(
        hash: Hash,
        state: Merkle,
        marker: &str,
    ) -> Result<Self, anyhow::Error> {
        let node_type = match marker {
            "C" => NodeType::Change,
            "T" => NodeType::Tag,
            _ => bail!("Invalid node type marker: {}", marker),
        };
        Ok(Self {
            hash,
            node_type,
            state,
        })
    }
}

pub async fn repository(
    repo: &Repository,
    self_path: Option<&Path>,
    // User name in case it isn't provided in the `name` argument already.
    user: Option<&str>,
    name: &str,
    channel: &str,
    no_cert_check: bool,
    with_path: bool,
) -> Result<RemoteRepo, anyhow::Error> {
    if let Some(name) = repo.config.remotes.iter().find(|e| e.name() == name) {
        name.to_remote(channel, no_cert_check, with_path).await
    } else {
        unknown_remote(self_path, user, name, channel, no_cert_check, with_path).await
    }
}

/// Associate a generated key with a remote identity. Patches authored
/// by unproven keys will only display the key as the author.
pub async fn prove(
    identity: &Complete,
    origin: Option<&str>,
    no_cert_check: bool,
) -> Result<(), anyhow::Error> {
    let remote = origin.unwrap_or(&identity.config.author.origin);
    let mut stderr = std::io::stderr();
    writeln!(
        stderr,
        "Linking identity `{}` with {}@{}",
        &identity.name, &identity.config.author.username, remote
    )?;

    let mut remote = if let Ok(repo) = Repository::find_root(None) {
        repository(
            &repo,
            None,
            Some(&identity.config.author.username),
            &remote,
            libatomic::DEFAULT_CHANNEL,
            no_cert_check,
            false,
        )
        .await?
    } else {
        unknown_remote(
            None,
            Some(&identity.config.author.username),
            &remote,
            libatomic::DEFAULT_CHANNEL,
            no_cert_check,
            false,
        )
        .await?
    };

    let (key, _password) = identity
        .credentials
        .clone()
        .unwrap()
        .decrypt(&identity.name)?;
    remote.prove(key).await?;

    Ok(())
}

#[async_trait]
pub trait ToRemote {
    async fn to_remote(
        &self,
        channel: &str,
        no_cert_check: bool,
        with_path: bool,
    ) -> Result<RemoteRepo, anyhow::Error>;
}

#[async_trait]
impl ToRemote for RemoteConfig {
    async fn to_remote(
        &self,
        channel: &str,
        no_cert_check: bool,
        with_path: bool,
    ) -> Result<RemoteRepo, anyhow::Error> {
        match self {
            RemoteConfig::Ssh { ssh, .. } => {
                if let Some(mut sshr) = ssh_remote(None, ssh, with_path) {
                    debug!("unknown_remote, ssh = {:?}", ssh);
                    if let Some(c) = sshr.connect(ssh, channel).await? {
                        return Ok(RemoteRepo::Ssh(c));
                    }
                }
                bail!("Remote not found: {:?}", ssh)
            }
            RemoteConfig::Http {
                http,
                headers,
                name,
            } => {
                let mut h = Vec::new();
                for (k, v) in headers.iter() {
                    match v {
                        RemoteHttpHeader::String(s) => {
                            h.push((k.clone(), s.clone()));
                        }
                        RemoteHttpHeader::Shell(shell) => {
                            h.push((k.clone(), shell_cmd(&shell.shell)?));
                        }
                    }
                }
                return Ok(RemoteRepo::Http(Http {
                    url: http.parse().unwrap(),
                    channel: channel.to_string(),
                    client: reqwest::ClientBuilder::new()
                        .danger_accept_invalid_certs(no_cert_check)
                        .build()?,
                    headers: h,
                    name: name.to_string(),
                }));
            }
        }
    }
}

pub async fn unknown_remote(
    self_path: Option<&Path>,
    user: Option<&str>,
    name: &str,
    channel: &str,
    no_cert_check: bool,
    with_path: bool,
) -> Result<RemoteRepo, anyhow::Error> {
    if let Ok(url) = url::Url::parse(name) {
        let scheme = url.scheme();
        if scheme == "http" || scheme == "https" {
            debug!("unknown_remote, http = {:?}", name);
            return Ok(RemoteRepo::Http(Http {
                url,
                channel: channel.to_string(),
                client: reqwest::ClientBuilder::new()
                    .danger_accept_invalid_certs(no_cert_check)
                    .build()?,
                headers: Vec::new(),
                name: name.to_string(),
            }));
        } else if scheme == "ssh" {
            if let Some(mut ssh) = ssh_remote(user, name, with_path) {
                debug!("unknown_remote, ssh = {:?}", ssh);
                if let Some(c) = ssh.connect(name, channel).await? {
                    return Ok(RemoteRepo::Ssh(c));
                }
            }
            bail!("Remote not found: {:?}", name)
        } else {
            bail!("Remote scheme not supported: {:?}", scheme)
        }
    }
    if let Ok(root) = std::fs::canonicalize(name) {
        if let Some(path) = self_path {
            let path = std::fs::canonicalize(path)?;
            if path == root {
                return Ok(RemoteRepo::LocalChannel(channel.to_string()));
            }
        }

        let mut dot_dir = root.join(DOT_DIR);
        let changes_dir = dot_dir.join(CHANGES_DIR);

        dot_dir.push(PRISTINE_DIR);
        debug!("dot_dir = {:?}", dot_dir);
        match libatomic::pristine::sanakirja::Pristine::new(&dot_dir.join("db")) {
            Ok(pristine) => {
                debug!("pristine done");
                return Ok(RemoteRepo::Local(Local {
                    root: Path::new(name).to_path_buf(),
                    channel: channel.to_string(),
                    changes_dir,
                    pristine: Arc::new(pristine),
                    name: name.to_string(),
                }));
            }
            Err(libatomic::pristine::sanakirja::SanakirjaError::Sanakirja(
                sanakirja::Error::IO(e),
            )) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!("repo not found")
            }
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(mut ssh) = ssh_remote(user, name, with_path) {
        debug!("unknown_remote, ssh = {:?}", ssh);
        if let Some(c) = ssh.connect(name, channel).await? {
            return Ok(RemoteRepo::Ssh(c));
        }
    }
    bail!("Remote not found: {:?}", name)
}

// Extracting this saves a little bit of duplication.
pub fn get_local_inodes(
    txn: &mut MutTxn<()>,
    channel: &ChannelRef<MutTxn<()>>,
    repo: &Repository,
    path: &[String],
) -> Result<HashSet<Position<NodeId>>, anyhow::Error> {
    let mut paths = HashSet::new();
    for path in path.iter() {
        let (p, ambiguous) = txn.follow_oldest_path(&repo.changes, &channel, path)?;
        if ambiguous {
            bail!("Ambiguous path: {:?}", path)
        }
        paths.insert(p);
        paths.extend(
            libatomic::fs::iter_graph_descendants(txn, &channel.read(), p)?.map(|x| x.unwrap()),
        );
    }
    Ok(paths)
}

/// Embellished [`RemoteDelta`] that has information specific
/// to a push operation. We want to know what our options are
/// for changes to upload, whether the remote has unrecorded relevant changes,
/// and whether the remote has changes we don't know about, since those might
/// effect whether or not we actually want to go through with the push.
pub struct PushDelta {
    pub to_upload: Vec<Node>,
    pub remote_unrecs: Vec<(u64, Node)>,
    pub unknown_changes: Vec<Node>,
}

/// For a [`RemoteRepo`] that's Local, Ssh, or Http
/// (anything other than a LocalChannel),
/// [`RemoteDelta`] contains data about the difference between
/// the "actual" state of the remote ('theirs') and the last version of it
/// that we cached ('ours'). The dichotomy is the last point at which the two
/// were the same. `remote_unrecs` is a list of changes which used to be
/// present in the remote, AND were present in the current channel we're
/// pulling to or pushing from. The significance of that is that if we knew
/// about a certain change but did not pull it, the user won't be notified
/// if it's unrecorded in the remote.
///
/// If the remote we're pulling from or pushing to is a LocalChannel,
/// (meaning it's just a different channel of the repo we're already in), then
/// `ours_ge_dichotomy`, `theirs_ge_dichotomy`, and `remote_unrecs` will be empty
/// since they have no meaning. If we're pulling from a LocalChannel,
/// there's no cache to have diverged from, and if we're pushing to a LocalChannel,
/// we're not going to suddenly be surprised by the presence of unknown changes.
///
/// This struct will be created by both a push and pull operation since both
/// need to update the changelist and will at least try to update the local
/// remote cache. For a push, this later gets turned into [`PushDelta`].
pub struct RemoteDelta<T: MutTxnTExt + TxnTExt> {
    pub inodes: HashSet<Position<Hash>>,
    pub to_download: Vec<Node>,
    pub remote_ref: Option<RemoteRef<T>>,
    pub ours_ge_dichotomy_set: HashSet<Node>,
    pub theirs_ge_dichotomy_set: HashSet<Node>,
    // Keep the Vec representation around as well so that notification
    // for unknown changes during shows the hashes in order.
    pub theirs_ge_dichotomy: Vec<(u64, Node)>,
    pub remote_unrecs: Vec<(u64, Node)>,
}

impl RemoteDelta<MutTxn<()>> {
    /// Make a [`PushDelta`] from a [`RemoteDelta`]
    /// when the remote is a [`RemoteRepo::LocalChannel`].
    pub fn to_local_channel_push(
        self,
        remote_channel: &str,
        txn: &mut MutTxn<()>,
        path: &[String],
        channel: &ChannelRef<MutTxn<()>>,
        repo: &Repository,
    ) -> Result<PushDelta, anyhow::Error> {
        let mut to_upload = Vec::new();
        let inodes = get_local_inodes(txn, channel, repo, path)?;

        for x in txn.reverse_log(&*channel.read(), None)? {
            let (_, (h, m)) = x?;
            if let Some(channel) = txn.load_channel(remote_channel)? {
                let channel = channel.read();
                let h_int = txn.get_internal(h)?.unwrap();
                if txn.get_changeset(txn.changes(&channel), h_int)?.is_none() {
                    let state: Merkle = m.into();
                    let node = Node::change(h.into(), state);
                    if inodes.is_empty() {
                        to_upload.push(node)
                    } else {
                        for p in inodes.iter() {
                            if txn.get_touched_files(p, Some(h_int))?.is_some() {
                                to_upload.push(node);
                                break;
                            }
                        }
                    }
                }
            }
        }
        assert!(self.ours_ge_dichotomy_set.is_empty());
        assert!(self.theirs_ge_dichotomy_set.is_empty());
        let d = PushDelta {
            to_upload: to_upload.into_iter().rev().collect(),
            remote_unrecs: self.remote_unrecs,
            unknown_changes: Vec::new(),
        };
        assert!(d.remote_unrecs.is_empty());
        Ok(d)
    }

    /// Make a [`PushDelta`] from a [`RemoteDelta`] when the remote
    /// is not a LocalChannel.
    pub fn to_remote_push(
        self,
        txn: &mut MutTxn<()>,
        path: &[String],
        channel: &ChannelRef<MutTxn<()>>,
        repo: &Repository,
    ) -> Result<PushDelta, anyhow::Error> {
        let mut to_upload = Vec::new();
        let inodes = get_local_inodes(txn, channel, repo, path)?;
        if let Some(ref remote_ref) = self.remote_ref {
            // Collect tags from channel's tags table
            let mut tags: HashSet<Merkle> = HashSet::new();
            let channel_read = channel.read();
            for tag_entry in txn.iter_tags(txn.tags(&*channel_read), 0)? {
                let (_, tag_bytes) = tag_entry?;
                // Extract merkle from tag
                let serialized = libatomic::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
                if let Ok(tag) = serialized.to_tag() {
                    debug!("Found local tag: {}", tag.state.to_base32());
                    tags.insert(tag.state);
                }
            }
            drop(channel_read);
            debug!("Found {} tags to potentially push", tags.len());
            debug!("Starting to iterate through channel log for push selection");
            for x in txn.reverse_log(&*channel.read(), None)? {
                let (_, (h, m)) = x?;
                debug!("Examining change: {:?}, state: {:?}", h, m);
                let state: Merkle = m.into();
                let change_node = Node::change(h.into(), state.clone());
                let h_unrecorded = self
                    .remote_unrecs
                    .iter()
                    .any(|(_, node)| node.hash == Hash::from(*h) && node.is_change());
                if !h_unrecorded {
                    if txn.remote_has_state(remote_ref, &m)?.is_some() {
                        debug!("remote_has_state: {:?}", m);
                        break;
                    }
                }
                let h_int = txn.get_internal(h)?.unwrap();
                let h_deser = Hash::from(h);
                // For elements that are in the uncached remote changes (theirs_ge_dichotomy),
                // don't put those in to_upload since the remote we're pushing to
                // already has those changes.
                if (!txn.remote_has_change(remote_ref, &h)? || h_unrecorded)
                    && !self.theirs_ge_dichotomy_set.contains(&change_node)
                {
                    if inodes.is_empty() {
                        if tags.remove(&m.into()) {
                            debug!("Adding tag state to upload: {:?}", m);
                            let tag_node = Node::tag(h_deser.clone(), state.clone());
                            to_upload.push(tag_node);
                        }
                        debug!("Adding change to upload: {:?}", h_deser);
                        to_upload.push(change_node.clone());
                    } else {
                        for p in inodes.iter() {
                            if txn.get_touched_files(p, Some(h_int))?.is_some() {
                                debug!("Adding change (with inode) to upload: {:?}", h_deser);
                                to_upload.push(change_node.clone());
                                if tags.remove(&m.into()) {
                                    debug!("Adding tag state (with inode) to upload: {:?}", m);
                                    let tag_node = Node::tag(h_deser.clone(), state.clone());
                                    to_upload.push(tag_node);
                                }
                                break;
                            }
                        }
                    }
                }
            }
            debug!("Processing remaining tags: {:?}", tags);
            for t in tags.iter() {
                if let Some(n) = txn.remote_has_state(&remote_ref, &t.into())? {
                    if !txn.is_tagged(&remote_ref.lock().tags, n)? {
                        debug!("Adding orphaned tag state to upload: {:?}", t);
                        let tag_hash = Hash::from(&SerializedMerkle::from(t));
                        let tag_node = Node::tag(tag_hash, *t);
                        to_upload.push(tag_node);
                    }
                } else {
                    // Remote doesn't have state yet, but push tag anyway
                    // Tags can be virtual dependencies that need to exist for validation
                    debug!("Remote doesn't have state {:?}, pushing tag anyway for dependency resolution", t);
                    let tag_hash = Hash::from(&SerializedMerkle::from(t));
                    let tag_node = Node::tag(tag_hash, *t);
                    to_upload.push(tag_node);
                }
            }
        }

        // { h | h \in theirs_ge_dichotomy /\ ~(h \in ours_ge_dichotomy) }
        // The set of their changes >= dichotomy that aren't
        // already known to our set of changes after the dichotomy.
        let mut unknown_changes = Vec::new();
        for (_, node) in self.theirs_ge_dichotomy.iter() {
            let h_is_known = txn.get_revchanges(&channel, &node.hash).unwrap().is_some();
            if !(self.ours_ge_dichotomy_set.contains(&node) || h_is_known) {
                unknown_changes.push(node.clone())
            }
            if node.is_tag() {
                let m_is_known = if let Some(n) = txn
                    .channel_has_state(txn.states(&*channel.read()), &node.state.into())
                    .unwrap()
                {
                    txn.is_tagged(txn.tags(&*channel.read()), n.into()).unwrap()
                } else {
                    false
                };
                if !m_is_known {
                    unknown_changes.push(node.clone())
                }
            }
        }

        debug!("Total items selected for upload: {}", to_upload.len());
        for node in to_upload.iter() {
            match node.node_type {
                NodeType::Change => debug!("  - Change: {}", node.hash.to_base32()),
                NodeType::Tag => debug!(
                    "  - Tag: {} (state: {})",
                    node.hash.to_base32(),
                    node.state.to_base32()
                ),
            }
        }

        Ok(PushDelta {
            to_upload: to_upload.into_iter().rev().collect(),
            remote_unrecs: self.remote_unrecs,
            unknown_changes,
        })
    }
}

/// Create a [`RemoteDelta`] for a [`RemoteRepo::LocalChannel`].
/// Since this case doesn't have a local remote cache to worry about,
/// mainly just calculates the `to_download` list of changes.
pub fn update_changelist_local_channel(
    remote_channel: &str,
    txn: &mut MutTxn<()>,
    path: &[String],
    current_channel: &ChannelRef<MutTxn<()>>,
    repo: &Repository,
    specific_changes: &[String],
) -> Result<RemoteDelta<MutTxn<()>>, anyhow::Error> {
    if !specific_changes.is_empty() {
        let mut to_download = Vec::new();
        for h in specific_changes {
            let h = txn.hash_from_prefix(h)?.0;
            if txn.get_revchanges(current_channel, &h)?.is_none() {
                // Get current state for the change node
                let state = txn.current_state(&*current_channel.read())?;
                to_download.push(Node::change(h, state));
            }
        }
        Ok(RemoteDelta {
            inodes: HashSet::new(),
            to_download,
            remote_ref: None,
            ours_ge_dichotomy_set: HashSet::new(),
            theirs_ge_dichotomy: Vec::new(),
            theirs_ge_dichotomy_set: HashSet::new(),
            remote_unrecs: Vec::new(),
        })
    } else {
        let mut inodes = HashSet::new();
        let inodes_ = get_local_inodes(txn, current_channel, repo, path)?;
        let mut to_download = Vec::new();
        inodes.extend(inodes_.iter().map(|x| libatomic::pristine::Position {
            change: txn.get_external(&x.change).unwrap().unwrap().into(),
            pos: x.pos,
        }));
        if let Some(remote_channel) = txn.load_channel(remote_channel)? {
            let remote_channel = remote_channel.read();
            for x in txn.reverse_log(&remote_channel, None)? {
                let (_, (h, m)) = x?;
                if txn
                    .channel_has_state(txn.states(&*current_channel.read()), &m)?
                    .is_some()
                {
                    break;
                }
                let h_int = txn.get_internal(h)?.unwrap();
                if txn
                    .get_changeset(txn.changes(&*current_channel.read()), h_int)?
                    .is_none()
                {
                    if inodes_.is_empty()
                        || inodes_.iter().any(|&inode| {
                            txn.get_rev_touched_files(h_int, Some(&inode))
                                .unwrap()
                                .is_some()
                        })
                    {
                        let state: Merkle = m.into();
                        to_download.push(Node::change(h.into(), state));
                    }
                }
            }
        }
        Ok(RemoteDelta {
            inodes,
            to_download,
            remote_ref: None,
            ours_ge_dichotomy_set: HashSet::new(),
            theirs_ge_dichotomy: Vec::new(),
            theirs_ge_dichotomy_set: HashSet::new(),
            remote_unrecs: Vec::new(),
        })
    }
}

impl RemoteRepo {
    fn name(&self) -> Option<&str> {
        match *self {
            RemoteRepo::Ssh(ref s) => Some(s.name.as_str()),
            RemoteRepo::Local(ref l) => Some(l.name.as_str()),
            RemoteRepo::Http(ref h) => Some(h.name.as_str()),
            RemoteRepo::LocalChannel(_) => None,
            RemoteRepo::None => unreachable!(),
        }
    }

    /// Get a node with its type from a remote position
    ///
    /// Phase 2: Node-type-aware remote operations
    /// This queries the node type from the database for a given remote entry.
    pub fn get_remote_node<T: TxnT>(
        txn: &T,
        remote: &RemoteRef<T>,
        position: u64,
    ) -> Result<Option<Node>, anyhow::Error> {
        let remote_lock = remote.lock();

        // Get hash and state from remote table
        if let Some((pos, pair)) = txn.get_remote_state(&remote_lock.remote, position)? {
            if pos == position {
                let hash: Hash = pair.a.into();
                let state: Merkle = pair.b.into();

                // Query node type from database
                if let Some(node_type) = txn.get_node_type_by_hash(&hash) {
                    return Ok(Some(match node_type {
                        NodeType::Change => Node::change(hash, state),
                        NodeType::Tag => Node::tag(hash, state),
                    }));
                } else {
                    debug!(
                        "Node type not found for hash {} at position {}, defaulting to Change",
                        hash.to_base32(),
                        position
                    );
                    // Default to Change if node type not registered
                    return Ok(Some(Node::change(hash, state)));
                }
            }
        }

        Ok(None)
    }

    /// Check if a remote entry is a tag
    ///
    /// Phase 2: Helper to determine if a remote position contains a tag node
    pub fn is_remote_tag<T: TxnT>(
        txn: &T,
        remote: &RemoteRef<T>,
        position: u64,
    ) -> Result<bool, anyhow::Error> {
        if let Some(node) = Self::get_remote_node(txn, remote, position)? {
            Ok(node.is_tag())
        } else {
            Ok(false)
        }
    }

    pub fn repo_name(&self) -> Result<Option<String>, anyhow::Error> {
        match *self {
            RemoteRepo::Ssh(ref s) => {
                if let Some(sep) = s.name.rfind(|c| c == ':' || c == '/') {
                    Ok(Some(s.name.split_at(sep + 1).1.to_string()))
                } else {
                    Ok(Some(s.name.as_str().to_string()))
                }
            }
            RemoteRepo::Local(ref l) => {
                if let Some(file) = l.root.file_name() {
                    Ok(Some(
                        file.to_str()
                            .context("failed to decode local repository name")?
                            .to_string(),
                    ))
                } else {
                    Ok(None)
                }
            }
            RemoteRepo::Http(ref h) => {
                if let Some(name) = libatomic::path::file_name(h.url.path()) {
                    if !name.trim().is_empty() {
                        return Ok(Some(name.trim().to_string()));
                    }
                }
                Ok(h.url.host().map(|h| h.to_string()))
            }
            RemoteRepo::LocalChannel(_) => Ok(None),
            RemoteRepo::None => unreachable!(),
        }
    }

    pub async fn finish(&mut self) -> Result<(), anyhow::Error> {
        if let RemoteRepo::Ssh(s) = self {
            s.finish().await?
        }
        Ok(())
    }

    pub async fn update_changelist<T: MutTxnTExt + TxnTExt + 'static>(
        &mut self,
        txn: &mut T,
        path: &[String],
    ) -> Result<Option<(HashSet<Position<Hash>>, RemoteRef<T>)>, anyhow::Error> {
        debug!("update_changelist");
        let id = if let Some(id) = self.get_id(txn).await? {
            id
        } else {
            return Ok(None);
        };
        let mut remote = if let Some(name) = self.name() {
            txn.open_or_create_remote(id, name)?
        } else {
            return Ok(None);
        };
        let n = self.dichotomy_changelist(txn, &remote.lock()).await?;
        debug!("update changelist {:?}", n);
        let v: Vec<_> = txn
            .iter_remote(&remote.lock().remote, n)?
            .filter_map(|k| {
                debug!("filter_map {:?}", k);
                let k = (*k.unwrap().0).into();
                if k >= n {
                    Some(k)
                } else {
                    None
                }
            })
            .collect();
        for k in v {
            debug!("deleting {:?}", k);
            txn.del_remote(&mut remote, k)?;
        }
        let v: Vec<_> = txn
            .iter_tags(&remote.lock().tags, n)?
            .filter_map(|k| {
                debug!("filter_map {:?}", k);
                let k = (*k.unwrap().0).into();
                if k >= n {
                    Some(k)
                } else {
                    None
                }
            })
            .collect();
        for k in v {
            debug!("deleting {:?}", k);
            txn.del_tags(&mut remote.lock().tags, k)?;
        }

        debug!("deleted");
        let paths = self.download_changelist(txn, &mut remote, n, path).await?;
        Ok(Some((paths, remote)))
    }

    async fn update_changelist_pushpull_from_scratch(
        &mut self,
        txn: &mut MutTxn<()>,
        path: &[String],
        current_channel: &ChannelRef<MutTxn<()>>,
    ) -> Result<RemoteDelta<MutTxn<()>>, anyhow::Error> {
        debug!("no id, starting from scratch");
        let (inodes, theirs_ge_dichotomy) = self.download_changelist_nocache(0, path).await?;
        let mut theirs_ge_dichotomy_set = HashSet::new();
        let mut to_download = Vec::new();
        let mut theirs_ge_dichotomy_nodes = Vec::new();

        for (pos, h, m, is_tag) in theirs_ge_dichotomy.iter() {
            let node = if *is_tag {
                Node::tag(*h, *m)
            } else {
                Node::change(*h, *m)
            };
            theirs_ge_dichotomy_set.insert(node);
            theirs_ge_dichotomy_nodes.push((*pos, node));

            if txn.get_revchanges(current_channel, h)?.is_none() {
                debug!("Adding change to download: {}", h.to_base32());
                to_download.push(Node::change(*h, *m));
            }
            if *is_tag {
                debug!(
                    "Processing tag: change={}, state={}",
                    h.to_base32(),
                    m.to_base32()
                );
                let ch = current_channel.read();
                if let Some(n) = txn.channel_has_state(txn.states(&*ch), &m.into())? {
                    debug!("Channel has state {} at position {}", m.to_base32(), n);
                    if !txn.is_tagged(txn.tags(&*ch), n.into())? {
                        debug!(
                            "State not tagged locally, adding to download: {}",
                            m.to_base32()
                        );
                        to_download.push(Node::tag(*h, *m));
                    } else {
                        debug!("State already tagged locally, skipping download");
                    }
                } else {
                    debug!(
                        "Channel doesn't have state, adding to download: {}",
                        m.to_base32()
                    );
                    to_download.push(Node::tag(*h, *m));
                }
            } else {
                debug!("Change {} is not a tag", h.to_base32());
            }
        }
        Ok(RemoteDelta {
            inodes,
            remote_ref: None,
            to_download,
            ours_ge_dichotomy_set: HashSet::new(),
            theirs_ge_dichotomy: theirs_ge_dichotomy_nodes,
            theirs_ge_dichotomy_set,
            remote_unrecs: Vec::new(),
        })
    }

    /// Creates a [`RemoteDelta`].
    ///
    /// IF:
    ///    the RemoteRepo is a [`RemoteRepo::LocalChannel`], delegate to
    ///    the simpler method [`update_changelist_local_channel`], returning the
    ///    `to_download` list of changes.
    ///
    /// ELSE:
    ///    calculate the `to_download` list of changes. Additionally, if there are
    ///    no remote unrecords, update the local remote cache. If there are remote unrecords,
    ///    calculate and return information about the difference between our cached version
    ///    of the remote, and their version of the remote.
    pub async fn update_changelist_pushpull(
        &mut self,
        txn: &mut MutTxn<()>,
        path: &[String],
        current_channel: &ChannelRef<MutTxn<()>>,
        force_cache: Option<bool>,
        repo: &Repository,
        specific_changes: &[String],
        is_pull: bool,
    ) -> Result<RemoteDelta<MutTxn<()>>, anyhow::Error> {
        debug!("update_changelist_pushpull");
        if let RemoteRepo::LocalChannel(c) = self {
            return update_changelist_local_channel(
                c,
                txn,
                path,
                current_channel,
                repo,
                specific_changes,
            );
        }

        let id = if let Some(id) = self.get_id(txn).await? {
            debug!("id = {:?}", id);
            id
        } else {
            return self
                .update_changelist_pushpull_from_scratch(txn, path, current_channel)
                .await;
        };
        let mut remote_ref = txn.open_or_create_remote(id, self.name().unwrap()).unwrap();
        let dichotomy_n = self.dichotomy_changelist(txn, &remote_ref.lock()).await?;
        let ours_ge_dichotomy: Vec<(u64, Node)> = txn
            .iter_remote(&remote_ref.lock().remote, dichotomy_n)?
            .filter_map(|k| {
                debug!("filter_map {:?}", k);
                match k.unwrap() {
                    (k, libatomic::pristine::Pair { a: hash, b: merkle }) => {
                        let (k, hash, state) =
                            (u64::from(*k), Hash::from(*hash), Merkle::from(*merkle));
                        if k >= dichotomy_n {
                            // Query node type from remote table if available
                            let node = Node::change(hash, state);
                            Some((k, node))
                        } else {
                            None
                        }
                    }
                }
            })
            .collect();
        let (inodes, theirs_ge_dichotomy) =
            self.download_changelist_nocache(dichotomy_n, path).await?;
        debug!("theirs_ge_dichotomy = {:?}", theirs_ge_dichotomy);
        let ours_ge_dichotomy_set = ours_ge_dichotomy
            .iter()
            .map(|(_, node)| node)
            .copied()
            .collect::<HashSet<Node>>();
        let mut theirs_ge_dichotomy_set = HashSet::new();
        let mut theirs_ge_dichotomy_nodes = Vec::new();
        for (pos, h, m, is_tag) in theirs_ge_dichotomy.iter() {
            let node = if *is_tag {
                Node::tag(*h, *m)
            } else {
                Node::change(*h, *m)
            };
            theirs_ge_dichotomy_set.insert(node);
            theirs_ge_dichotomy_nodes.push((*pos, node));
        }

        // remote_unrecs = {x: (u64, Hash) | x \in ours_ge_dichot /\ ~(x \in theirs_ge_dichot) /\ x \in current_channel }
        let remote_unrecs = remote_unrecs(
            txn,
            current_channel,
            &ours_ge_dichotomy,
            &theirs_ge_dichotomy_set,
        )?;
        let should_cache = if let Some(true) = force_cache {
            true
        } else {
            remote_unrecs.is_empty()
        };
        debug!(
            "should_cache = {:?} {:?} {:?}",
            force_cache, remote_unrecs, should_cache
        );
        if should_cache {
            use libatomic::ChannelMutTxnT;
            for (k, node) in ours_ge_dichotomy.iter().copied() {
                match node.node_type {
                    NodeType::Tag => txn.del_tags(&mut remote_ref.lock().tags, k)?,
                    NodeType::Change => {
                        txn.del_remote(&mut remote_ref, k)?;
                    }
                }
            }
            for (n, node) in theirs_ge_dichotomy_nodes.iter().copied() {
                debug!("theirs: {:?} {:?} {:?}", n, node.hash, node.state);
                txn.put_remote(&mut remote_ref, n, (node.hash, node.state))?;
                if node.is_tag() {
                    txn.put_tags(&mut remote_ref.lock().tags, n, &node.state)?;
                }
            }
        }
        if !specific_changes.is_empty() {
            // Here, the user only wanted to push/pull specific changes
            let to_download = specific_changes
                .iter()
                .map(|h| {
                    if is_pull {
                        {
                            if let Ok(t) = txn.state_from_prefix(&remote_ref.lock().states, h) {
                                let tag_hash = Hash::from(&SerializedMerkle::from(&t.0));
                                return Ok(Node::tag(tag_hash, t.0));
                            }
                        }
                        let hash = txn.hash_from_prefix_remote(&remote_ref, h)?;
                        let state = txn.current_state(&*current_channel.read())?;
                        Ok(Node::change(hash, state))
                    } else {
                        if let Ok(t) = txn.state_from_prefix(&current_channel.read().states, h) {
                            let tag_hash = Hash::from(&SerializedMerkle::from(&t.0));
                            Ok(Node::tag(tag_hash, t.0))
                        } else {
                            let hash = txn.hash_from_prefix(h)?.0;
                            let state = txn.current_state(&*current_channel.read())?;
                            Ok(Node::change(hash, state))
                        }
                    }
                })
                .collect::<Result<Vec<_>, anyhow::Error>>();
            Ok(RemoteDelta {
                inodes,
                remote_ref: Some(remote_ref),
                to_download: to_download?,
                ours_ge_dichotomy_set,
                theirs_ge_dichotomy: theirs_ge_dichotomy_nodes,
                theirs_ge_dichotomy_set,
                remote_unrecs,
            })
        } else {
            let mut to_download: Vec<Node> = Vec::new();
            let mut to_download_ = HashSet::new();
            for x in txn.iter_rev_remote(&remote_ref.lock().remote, None)? {
                let (_, p) = x?;
                let h: Hash = p.a.into();
                if txn
                    .channel_has_state(txn.states(&current_channel.read()), &p.b)
                    .unwrap()
                    .is_some()
                {
                    break;
                }
                if txn.get_revchanges(&current_channel, &h).unwrap().is_none() {
                    let state: Merkle = p.b.into();
                    let node = Node::change(h, state);
                    if to_download_.insert(node.clone()) {
                        to_download.push(node);
                    }
                }
            }

            // The patches in theirs_ge_dichotomy are unknown to us,
            // download them.
            for (n, node) in theirs_ge_dichotomy_nodes.iter() {
                debug!(
                    "update_changelist_pushpull line {}, {:?} {:?}",
                    line!(),
                    n,
                    node.hash
                );
                // In all cases, add this new change/state/tag to `to_download`.
                if txn
                    .get_revchanges(&current_channel, &node.hash)
                    .unwrap()
                    .is_none()
                {
                    if to_download_.insert(node.clone()) {
                        to_download.push(node.clone());
                    }
                    if node.is_tag() {
                        to_download.push(node.clone());
                    }
                } else if node.is_tag() {
                    let has_tag = if let Some(n) = txn.channel_has_state(
                        txn.states(&*current_channel.read()),
                        &node.state.into(),
                    )? {
                        txn.is_tagged(txn.tags(&current_channel.read()), n.into())?
                    } else {
                        false
                    };
                    if !has_tag {
                        to_download.push(node.clone());
                    }
                }
                // Additionally, if there are no remote unrecords
                // (i.e. if `should_cache`), cache.
                if should_cache && ours_ge_dichotomy_set.get(&node).is_none() {
                    use libatomic::ChannelMutTxnT;
                    txn.put_remote(&mut remote_ref, *n, (node.hash, node.state))?;
                    if node.is_tag() {
                        let mut rem = remote_ref.lock();
                        txn.put_tags(&mut rem.tags, *n, &node.state)?;
                    }
                }
            }
            Ok(RemoteDelta {
                inodes,
                remote_ref: Some(remote_ref),
                to_download: to_download.into_iter().rev().collect(),
                ours_ge_dichotomy_set,
                theirs_ge_dichotomy: theirs_ge_dichotomy_nodes,
                theirs_ge_dichotomy_set,
                remote_unrecs,
            })
        }
    }

    /// Get the list of the remote's changes that come after `from: u64`.
    /// Instead of immediately updating the local cache of the remote, return
    /// the change info without changing the cache.
    pub async fn download_changelist_nocache(
        &mut self,
        from: u64,
        paths: &[String],
    ) -> Result<(HashSet<Position<Hash>>, Vec<(u64, Hash, Merkle, bool)>), anyhow::Error> {
        let mut v = Vec::new();
        let f = |v: &mut Vec<(u64, Hash, Merkle, bool)>, n, h, m, m2| {
            debug!("no cache: {:?}", h);
            Ok(v.push((n, h, m, m2)))
        };
        let r = match *self {
            RemoteRepo::Local(ref mut l) => l.download_changelist(f, &mut v, from, paths)?,
            RemoteRepo::Ssh(ref mut s) => s.download_changelist(f, &mut v, from, paths).await?,
            RemoteRepo::Http(ref h) => h.download_changelist(f, &mut v, from, paths).await?,
            RemoteRepo::LocalChannel(_) => HashSet::new(),
            RemoteRepo::None => unreachable!(),
        };
        Ok((r, v))
    }

    /// Uses a binary search to find the integer identifier of the last point
    /// at which our locally cached version of the remote was the same as the 'actual'
    /// state of the remote.
    async fn dichotomy_changelist<T: MutTxnT + TxnTExt>(
        &mut self,
        txn: &T,
        remote: &libatomic::pristine::Remote<T>,
    ) -> Result<u64, anyhow::Error> {
        let mut a = 0;
        let (mut b, state): (_, Merkle) = if let Some((u, v)) = txn.last_remote(&remote.remote)? {
            debug!("dichotomy_changelist: {:?} {:?}", u, v);
            (u, (&v.b).into())
        } else {
            debug!("the local copy of the remote has no changes");
            return Ok(0);
        };
        let last_statet = if let Some((_, _, v)) = txn.last_remote_tag(&remote.tags)? {
            v.into()
        } else {
            Merkle::zero()
        };
        debug!("last_state: {:?} {:?}", state, last_statet);
        if let Some((_, s, st)) = self.get_state(txn, Some(b)).await? {
            debug!("remote last_state: {:?} {:?}", s, st);
            if s == state && st == last_statet {
                // The local list is already up to date.
                return Ok(b + 1);
            }
        }
        // Else, find the last state we have in common with the
        // remote, it might be older than the last known state (if
        // changes were unrecorded on the remote).
        while a < b {
            let mid = (a + b) / 2;
            let (mid, state) = {
                let (a, b) = txn.get_remote_state(&remote.remote, mid)?.unwrap();
                (a, b.b)
            };
            let statet = if let Some((_, b)) = txn.get_remote_tag(&remote.tags, mid)? {
                // There's still a tag at position >= mid in the
                // sequence.
                b.b.into()
            } else {
                // No tag at or after mid, the last state, `statet`,
                // is the right answer in that case.
                last_statet
            };

            let remote_state = self.get_state(txn, Some(mid)).await?;
            debug!("dichotomy {:?} {:?} {:?}", mid, state, remote_state);
            if let Some((_, remote_state, remote_statet)) = remote_state {
                if remote_state == state && remote_statet == statet {
                    if a == mid {
                        return Ok(a + 1);
                    } else {
                        a = mid;
                        continue;
                    }
                }
            }
            if b == mid {
                break;
            } else {
                b = mid
            }
        }
        Ok(a)
    }

    async fn get_state<T: libatomic::TxnTExt>(
        &mut self,
        txn: &T,
        mid: Option<u64>,
    ) -> Result<Option<(u64, Merkle, Merkle)>, anyhow::Error> {
        match *self {
            RemoteRepo::Local(ref mut l) => l.get_state(mid),
            RemoteRepo::Ssh(ref mut s) => s.get_state(mid).await,
            RemoteRepo::Http(ref mut h) => h.get_state(mid).await,
            RemoteRepo::LocalChannel(ref channel) => {
                if let Some(channel) = txn.load_channel(&channel)? {
                    local::get_state(txn, &channel, mid)
                } else {
                    Ok(None)
                }
            }
            RemoteRepo::None => unreachable!(),
        }
    }

    /// This method might return `Ok(None)` in some cases, for example
    /// if the remote wants to indicate not to store a cache. This is
    /// the case for Nest channels, for example.
    async fn get_id<T: libatomic::TxnTExt + 'static>(
        &mut self,
        txn: &T,
    ) -> Result<Option<libatomic::pristine::RemoteId>, anyhow::Error> {
        match *self {
            RemoteRepo::Local(ref l) => Ok(Some(l.get_id()?)),
            RemoteRepo::Ssh(ref mut s) => s.get_id().await,
            RemoteRepo::Http(ref h) => h.get_id().await,
            RemoteRepo::LocalChannel(ref channel) => {
                if let Some(channel) = txn.load_channel(&channel)? {
                    Ok(txn.id(&*channel.read()).cloned())
                } else {
                    Err(anyhow::anyhow!(
                        "Unable to retrieve RemoteId for LocalChannel remote"
                    ))
                }
            }
            RemoteRepo::None => unreachable!(),
        }
    }

    pub async fn archive<W: std::io::Write + Send + 'static>(
        &mut self,
        prefix: Option<String>,
        state: Option<(Merkle, &[Hash])>,
        umask: u16,
        w: W,
    ) -> Result<u64, anyhow::Error> {
        match *self {
            RemoteRepo::Local(ref mut l) => {
                debug!("archiving local repo");
                let changes = libatomic::changestore::filesystem::FileSystem::from_root(
                    &l.root,
                    atomic_repository::max_files()?,
                );
                let mut tarball = libatomic::output::Tarball::new(w, prefix, umask);
                let conflicts = if let Some((state, extra)) = state {
                    let txn = l.pristine.arc_txn_begin()?;
                    let channel = {
                        let txn = txn.read();
                        txn.load_channel(&l.channel)?.unwrap()
                    };
                    txn.archive_with_state(&changes, &channel, &state, extra, &mut tarball, 0)?
                } else {
                    let txn = l.pristine.arc_txn_begin()?;
                    let channel = {
                        let txn = txn.read();
                        txn.load_channel(&l.channel)?.unwrap()
                    };
                    txn.archive(&changes, &channel, &mut tarball)?
                };
                Ok(conflicts.len() as u64)
            }
            RemoteRepo::Ssh(ref mut s) => s.archive(prefix, state, w).await,
            RemoteRepo::Http(ref mut h) => h.archive(prefix, state, w).await,
            RemoteRepo::LocalChannel(_) => unreachable!(),
            RemoteRepo::None => unreachable!(),
        }
    }

    async fn download_changelist<T: MutTxnTExt>(
        &mut self,
        txn: &mut T,
        remote: &mut RemoteRef<T>,
        from: u64,
        paths: &[String],
    ) -> Result<HashSet<Position<Hash>>, anyhow::Error> {
        let f = |a: &mut (&mut T, &mut RemoteRef<T>), n, h, m, is_tag| {
            let (ref mut txn, ref mut remote) = *a;
            txn.put_remote(remote, n, (h, m))?;
            if is_tag {
                txn.put_tags(&mut remote.lock().tags, n, &m.into())?;
            }
            Ok(())
        };
        match *self {
            RemoteRepo::Local(ref mut l) => {
                l.download_changelist(f, &mut (txn, remote), from, paths)
            }
            RemoteRepo::Ssh(ref mut s) => {
                s.download_changelist(f, &mut (txn, remote), from, paths)
                    .await
            }
            RemoteRepo::Http(ref h) => {
                h.download_changelist(f, &mut (txn, remote), from, paths)
                    .await
            }
            RemoteRepo::LocalChannel(_) => Ok(HashSet::new()),
            RemoteRepo::None => unreachable!(),
        }
    }

    pub async fn upload_nodes<T: MutTxnTExt + 'static>(
        &mut self,
        txn: &mut T,
        local: PathBuf,
        to_channel: Option<&str>,
        nodes: &[Node],
    ) -> Result<(), anyhow::Error> {
        let upload_bar = ProgressBar::new(nodes.len() as u64, UPLOAD_MESSAGE)?;

        match self {
            RemoteRepo::Local(ref mut l) => l.upload_nodes(upload_bar, local, to_channel, nodes)?,
            RemoteRepo::Ssh(ref mut s) => {
                s.upload_nodes(upload_bar, local, to_channel, nodes).await?
            }
            RemoteRepo::Http(ref mut h) => {
                h.upload_nodes(upload_bar, local, to_channel, nodes).await?
            }
            RemoteRepo::LocalChannel(ref channel) => {
                let mut channel = txn.open_or_create_channel(channel)?;
                let store = libatomic::changestore::filesystem::FileSystem::from_changes(
                    local,
                    atomic_repository::max_files()?,
                );
                local::upload_nodes(upload_bar, &store, txn, &mut channel, nodes)?
            }
            RemoteRepo::None => unreachable!(),
        }
        Ok(())
    }

    /// Start (and possibly complete) the download of a node.
    pub async fn download_nodes(
        &mut self,
        progress_bar: ProgressBar,
        nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
        send: &mut tokio::sync::mpsc::Sender<(Node, bool)>,
        path: &mut PathBuf,
        full: bool,
    ) -> Result<bool, anyhow::Error> {
        debug!("download_nodes");
        match *self {
            RemoteRepo::Local(ref mut l) => {
                l.download_nodes(progress_bar, nodes, send, path).await?
            }
            RemoteRepo::Ssh(ref mut s) => {
                s.download_nodes(progress_bar, nodes, send, path, full)
                    .await?
            }
            RemoteRepo::Http(ref mut h) => {
                h.download_nodes(progress_bar, nodes, send, path, full)
                    .await?
            }
            RemoteRepo::LocalChannel(_) => {
                while let Some(node) = nodes.recv().await {
                    send.send((node, true)).await?;
                }
            }
            RemoteRepo::None => unreachable!(),
        }
        Ok(true)
    }

    pub async fn update_identities<T: MutTxnTExt + TxnTExt + GraphIter>(
        &mut self,
        repo: &mut Repository,
        remote: &RemoteRef<T>,
    ) -> Result<(), anyhow::Error> {
        debug!("Downloading identities");
        let mut id_path = repo.path.clone();
        id_path.push(DOT_DIR);
        id_path.push("identities");
        let rev = None;
        let r = match *self {
            RemoteRepo::Local(ref mut l) => l.update_identities(rev, id_path).await?,
            RemoteRepo::Ssh(ref mut s) => s.update_identities(rev, id_path).await?,
            RemoteRepo::Http(ref mut h) => h.update_identities(rev, id_path).await?,
            RemoteRepo::LocalChannel(_) => 0,
            RemoteRepo::None => unreachable!(),
        };
        remote.set_id_revision(r);
        Ok(())
    }

    pub async fn prove(&mut self, key: libatomic::key::SKey) -> Result<(), anyhow::Error> {
        match *self {
            RemoteRepo::Ssh(ref mut s) => s.prove(key).await,
            RemoteRepo::Http(ref mut h) => h.prove(key).await,
            RemoteRepo::None => unreachable!(),
            _ => Ok(()),
        }
    }

    pub async fn pull<T: MutTxnTExt + TxnTExt + GraphIter + 'static>(
        &mut self,
        repo: &mut Repository,
        txn: &mut T,
        channel: &mut ChannelRef<T>,
        to_apply: &[Node],
        inodes: &HashSet<Position<Hash>>,
        do_apply: bool,
    ) -> Result<Vec<Node>, anyhow::Error> {
        let apply_len = to_apply.len() as u64;
        let download_bar = ProgressBar::new(apply_len, DOWNLOAD_MESSAGE)?;
        let apply_bar = if do_apply {
            Some(ProgressBar::new(apply_len, APPLY_MESSAGE)?)
        } else {
            None
        };

        let (mut send, recv) = tokio::sync::mpsc::channel(100);

        let mut self_ = std::mem::replace(self, RemoteRepo::None);
        let (hash_send, mut hash_recv) = tokio::sync::mpsc::unbounded_channel();
        let mut change_path_ = repo.path.clone();
        change_path_.push(DOT_DIR);
        change_path_.push("changes");
        let cloned_download_bar = download_bar.clone();
        let t = tokio::spawn(async move {
            self_
                .download_nodes(
                    cloned_download_bar,
                    &mut hash_recv,
                    &mut send,
                    &mut change_path_,
                    false,
                )
                .await?;

            Ok::<_, anyhow::Error>(self_)
        });

        let mut change_path_ = repo.changes_dir.clone();
        let mut waiting = 0;
        let (send_ready, mut recv_ready) = tokio::sync::mpsc::channel(100);

        let mut asked = HashSet::new();
        for node in to_apply {
            debug!("to_apply {:?}", node);
            match node.node_type {
                NodeType::Change => {
                    libatomic::changestore::filesystem::push_filename(
                        &mut change_path_,
                        &node.hash,
                    );
                }
                NodeType::Tag => {
                    libatomic::changestore::filesystem::push_tag_filename(
                        &mut change_path_,
                        &node.state,
                    );
                }
            }
            asked.insert(*node);
            hash_send.send(*node)?;
            waiting += 1;
            libatomic::changestore::filesystem::pop_filename(&mut change_path_);
        }

        let u = self
            .download_changes_rec(
                repo,
                hash_send,
                recv,
                send_ready,
                download_bar,
                waiting,
                asked,
            )
            .await?;

        let mut ws = libatomic::ApplyWorkspace::new();
        let mut to_apply_inodes = HashSet::new();
        while let Some(node) = recv_ready.recv().await {
            debug!("to_apply: {:?}", node);
            let touches_inodes = match node.node_type {
                NodeType::Tag => {
                    // Tags should always be applied when inodes is empty (pulling everything)
                    inodes.is_empty()
                }
                NodeType::Change => {
                    inodes.is_empty()
                        || {
                            debug!("inodes = {:?}", inodes);
                            use libatomic::changestore::ChangeStore;
                            let changes = repo.changes.get_changes(&node.hash)?;
                            changes.iter().any(|c| {
                                c.iter().any(|c| {
                                    let inode = c.inode();
                                    debug!("inode = {:?}", inode);
                                    let any_match = inodes.contains(&Position {
                                        change: inode.change.unwrap_or(node.hash),
                                        pos: inode.pos,
                                    });
                                    any_match
                                })
                            })
                        }
                        || { inodes.iter().any(|i| i.change == node.hash) }
                }
            };

            if touches_inodes {
                to_apply_inodes.insert(node);
            } else {
                continue;
            }

            if let Some(apply_bar) = apply_bar.clone() {
                info!("Applying {:?}", node);
                apply_bar.inc(1);
                debug!("apply");
                // Use unified apply for both changes and tags
                let mut channel = channel.write();
                txn.apply_node_rec_ws(
                    &repo.changes,
                    &mut channel,
                    &node.hash,
                    node.node_type,
                    &mut ws,
                )?;

                // If it's a tag, store consolidating metadata
                if node.node_type == NodeType::Tag {
                    let serialized_state: libatomic::pristine::SerializedMerkle =
                        (&node.state).into();
                    if let Some(_n) =
                        txn.channel_has_state(txn.states(&*channel), &serialized_state)?
                    {
                        // Tag file reading removed - breaking change for MVP
                        // Tags must be regenerated in new format
                        // Use current timestamp since we can't read tag files
                        let original_timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        // Calculate consolidating tag metadata
                        let start_position = {
                            let mut last_tag_pos = None;
                            for entry in txn.rev_iter_tags(txn.tags(&*channel), None)? {
                                let (pos, _merkle_pair) = entry?;
                                debug!("Found previous tag at position: {:?}", pos);
                                last_tag_pos = Some(pos);
                                break;
                            }
                            last_tag_pos.map(|p| p.0 + 1).unwrap_or(0)
                        };

                        // Collect changes from last tag onwards
                        let mut consolidated_changes = Vec::new();
                        let mut change_count = 0u64;

                        for entry in txn.log(&*channel, start_position)? {
                            let (pos, (hash, _)) = entry?;
                            let hash: libatomic::pristine::Hash = hash.into();
                            debug!("  Position {}: including change {}", pos, hash.to_base32());
                            consolidated_changes.push(hash);
                            change_count += 1;
                        }

                        debug!(
                            "Tag consolidation: {} changes since position {}",
                            change_count, start_position
                        );

                        let dependency_count_before = change_count;
                        let consolidated_change_count = change_count;

                        // Get channel name
                        let channel_name = txn.name(&*channel).to_string();

                        // Create consolidating tag metadata with original timestamp
                        // Hash IS Merkle now, so we can use it directly
                        let tag_hash = node.state;
                        let mut tag = libatomic::pristine::Tag::new(
                            tag_hash,
                            node.state,
                            channel_name,
                            None,
                            dependency_count_before,
                            consolidated_change_count,
                            consolidated_changes,
                        );
                        tag.consolidation_timestamp = original_timestamp;
                        // Set the change_file_hash to the merkle state
                        // This is what should be used as a dependency when recording changes after the tag
                        tag.change_file_hash = Some(node.state);

                        // Serialize and store consolidating tag metadata
                        let serialized = libatomic::pristine::SerializedTag::from_tag(&tag)?;

                        debug!("Storing consolidating tag metadata");
                        txn.put_tag(&tag_hash, &serialized)?;
                        debug!(
                            "Tagged state {} with consolidating metadata",
                            node.state.to_base32()
                        );
                    } else {
                        debug!(
                            "Warning: Cannot add tag metadata {}: channel does not have that state yet",
                            node.state.to_base32()
                        );
                    }
                }
                debug!("applied");
            } else {
                debug!("not applying {:?}", node)
            }
        }

        let mut result = Vec::with_capacity(to_apply_inodes.len());
        for h in to_apply {
            if to_apply_inodes.contains(&h) {
                result.push(*h)
            }
        }

        debug!("finished");
        debug!("waiting for spawned process");
        *self = t.await??;
        u.await??;
        Ok(result)
    }

    async fn download_changes_rec(
        &mut self,
        repo: &mut Repository,
        send_hash: tokio::sync::mpsc::UnboundedSender<Node>,
        mut recv_signal: tokio::sync::mpsc::Receiver<(Node, bool)>,
        send_ready: tokio::sync::mpsc::Sender<Node>,
        progress_bar: ProgressBar,
        mut waiting: usize,
        mut asked: HashSet<Node>,
    ) -> Result<tokio::task::JoinHandle<Result<(), anyhow::Error>>, anyhow::Error> {
        let mut dep_path = repo.changes_dir.clone();
        let changes = repo.changes.clone();
        let t = tokio::spawn(async move {
            if waiting == 0 {
                return Ok(());
            }
            let mut ready = Vec::new();
            while let Some((node, follow)) = recv_signal.recv().await {
                debug!("received {:?} {:?}", node, follow);
                match node.node_type {
                    NodeType::Change => {
                        waiting -= 1;
                        if follow {
                            use libatomic::changestore::ChangeStore;
                            let mut needs_dep = false;
                            for dep in changes.get_dependencies(&node.hash)? {
                                let dep: libatomic::pristine::Hash = dep;

                                libatomic::changestore::filesystem::push_filename(
                                    &mut dep_path,
                                    &dep,
                                );
                                let has_dep = std::fs::metadata(&dep_path).is_ok();
                                libatomic::changestore::filesystem::pop_filename(&mut dep_path);

                                if !has_dep {
                                    needs_dep = true;
                                    let dep_node = Node::change(dep, node.state.clone());
                                    if asked.insert(dep_node.clone()) {
                                        progress_bar.inc(1);
                                        send_hash.send(dep_node)?;
                                        waiting += 1
                                    }
                                }
                            }

                            if !needs_dep {
                                send_ready.send(node.clone()).await?;
                            } else {
                                ready.push(node.clone())
                            }
                        } else {
                            send_ready.send(node.clone()).await?;
                        }
                    }
                    NodeType::Tag => {
                        // Tag state files don't have dependencies, send immediately
                        waiting -= 1;
                        debug!("received tag state {:?}, sending to ready", node.state);
                        send_ready.send(node.clone()).await?;
                    }
                }
                if waiting == 0 {
                    break;
                }
            }
            info!("waiting loop done");
            for r in ready {
                send_ready.send(r).await?;
            }
            std::mem::drop(recv_signal);
            Ok(())
        });
        Ok(t)
    }

    pub async fn clone_tag<T: MutTxnTExt + TxnTExt + GraphIter + 'static>(
        &mut self,
        repo: &mut Repository,
        txn: &mut T,
        channel: &mut ChannelRef<T>,
        tag: &[Hash],
    ) -> Result<(), anyhow::Error> {
        let (send_hash, mut recv_hash) = tokio::sync::mpsc::unbounded_channel();
        let (mut send_signal, recv_signal) = tokio::sync::mpsc::channel(100);
        let mut self_ = std::mem::replace(self, RemoteRepo::None);
        let mut change_path_ = repo.changes_dir.clone();
        let download_bar = ProgressBar::new(tag.len() as u64, DOWNLOAD_MESSAGE)?;
        let cloned_download_bar = download_bar.clone();

        let t = tokio::spawn(async move {
            self_
                .download_nodes(
                    cloned_download_bar,
                    &mut recv_hash,
                    &mut send_signal,
                    &mut change_path_,
                    false,
                )
                .await?;
            Ok(self_)
        });

        let mut waiting = 0;
        let mut asked = HashSet::new();
        for &h in tag.iter() {
            waiting += 1;
            send_hash.send(Node::change(h, Merkle::zero()))?;
            asked.insert(Node::change(h, Merkle::zero()));
        }

        let (send_ready, mut recv_ready) = tokio::sync::mpsc::channel(100);

        let u = self
            .download_changes_rec(
                repo,
                send_hash,
                recv_signal,
                send_ready,
                download_bar,
                waiting,
                asked,
            )
            .await?;

        let mut hashes = Vec::new();
        let mut ws = libatomic::ApplyWorkspace::new();
        {
            let mut channel_ = channel.write();
            while let Some(node) = recv_ready.recv().await {
                // Use unified apply for both changes and tags
                txn.apply_node_rec_ws(
                    &repo.changes,
                    &mut channel_,
                    &node.hash,
                    node.node_type,
                    &mut ws,
                )?;
                hashes.push(node);
            }
        }
        let r: Result<_, anyhow::Error> = t.await?;
        *self = r?;
        u.await??;
        self.complete_changes(repo, txn, channel, &hashes, false)
            .await?;
        Ok(())
    }

    pub async fn clone_state<T: MutTxnTExt + TxnTExt + GraphIter + 'static>(
        &mut self,
        repo: &mut Repository,
        txn: &mut T,
        channel: &mut ChannelRef<T>,
        state: Merkle,
        _changes: &[Node],
    ) -> Result<(), anyhow::Error> {
        let id = if let Some(id) = self.get_id(txn).await? {
            id
        } else {
            return Ok(());
        };
        self.update_changelist(txn, &[]).await?;
        let remote = txn.open_or_create_remote(id, self.name().unwrap()).unwrap();
        let mut to_pull = Vec::new();
        let mut found = false;
        for x in txn.iter_remote(&remote.lock().remote, 0)? {
            let (n, p) = x?;
            debug!("{:?} {:?}", n, p);
            to_pull.push(Node::change(p.a.into(), p.b.into()));
            if p.b == state {
                found = true;
                break;
            }
        }
        if !found {
            bail!("State not found: {:?}", state)
        }
        self.pull(repo, txn, channel, &to_pull, &HashSet::new(), true)
            .await?;
        self.update_identities(repo, &remote).await?;

        self.complete_changes(repo, txn, channel, &to_pull, false)
            .await?;
        Ok(())
    }

    pub async fn complete_changes<T: MutTxnT + TxnTExt + GraphIter>(
        &mut self,
        repo: &atomic_repository::Repository,
        txn: &T,
        local_channel: &mut ChannelRef<T>,
        nodes: &[Node],
        full: bool,
    ) -> Result<(), anyhow::Error> {
        debug!("complete nodes {:?}", nodes);
        use libatomic::changestore::ChangeStore;
        let (send_hash, mut recv_hash) = tokio::sync::mpsc::unbounded_channel();
        let (mut send_sig, mut recv_sig) = tokio::sync::mpsc::channel(100);
        let mut self_ = std::mem::replace(self, RemoteRepo::None);
        let mut changes_dir = repo.changes_dir.clone();

        let download_bar = ProgressBar::new(nodes.len() as u64, DOWNLOAD_MESSAGE)?;
        let _completion_spinner = Spinner::new(COMPLETE_MESSAGE)?;
        let t: tokio::task::JoinHandle<Result<RemoteRepo, anyhow::Error>> =
            tokio::spawn(async move {
                self_
                    .download_nodes(
                        download_bar,
                        &mut recv_hash,
                        &mut send_sig,
                        &mut changes_dir,
                        true,
                    )
                    .await?;
                Ok::<_, anyhow::Error>(self_)
            });

        for node in nodes {
            if node.is_tag() {
                continue; // Skip tags - they should not be downloaded, will be regenerated
            }
            let sc = (&node.hash).into();

            if let Some(internal) = txn.get_internal(&sc)? {
                if let Some(node_type) = txn.get_node_type(internal)? {
                    if node_type == libatomic::pristine::NodeType::Tag {
                        debug!("Skipping tag {} in complete_changes", node.hash.to_base32());
                        continue;
                    }
                }
            }
            if repo
                .changes
                .has_contents(node.hash, txn.get_internal(&sc)?.cloned())
            {
                debug!("has contents {:?}", node.hash);
                continue;
            }
            if full {
                debug!("sending send_hash");
                send_hash.send(node.clone())?;
                debug!("sent");
                continue;
            }
            let change = if let Some(&i) = txn.get_internal(&sc)? {
                i
            } else {
                debug!("could not find internal for {:?}", sc);
                continue;
            };
            // Check if at least one non-empty vertex from c is still alive.
            let v = libatomic::pristine::Vertex {
                change,
                start: libatomic::pristine::ChangePosition(0u64.into()),
                end: libatomic::pristine::ChangePosition(0u64.into()),
            };
            let channel = local_channel.read();
            let graph = txn.graph(&channel);
            for x in txn.iter_graph(graph, Some(&v))? {
                let (v, e) = x?;
                if v.change > change {
                    break;
                } else if e.flag().is_alive_parent() {
                    send_hash.send(node.clone())?;
                    break;
                }
            }
        }
        debug!("dropping send_hash");
        std::mem::drop(send_hash);
        while recv_sig.recv().await.is_some() {}
        *self = t.await??;
        Ok(())
    }

    pub async fn clone_channel<T: MutTxnTExt + TxnTExt + GraphIter + 'static>(
        &mut self,
        repo: &mut Repository,
        txn: &mut T,
        local_channel: &mut ChannelRef<T>,
        path: &[String],
    ) -> Result<(), anyhow::Error> {
        let (inodes, remote_changes) = if let Some(x) = self.update_changelist(txn, path).await? {
            x
        } else {
            bail!("Channel not found")
        };
        let mut pullable = Vec::new();
        {
            let rem = remote_changes.lock();
            for x in txn.iter_remote(&rem.remote, 0)? {
                let (_, p) = x?;
                pullable.push(Node::change(p.a.into(), p.b.into()))
            }
            debug!(
                "Built pullable list: {} items (will filter tags after pull)",
                pullable.len()
            );
        }
        self.pull(repo, txn, local_channel, &pullable, &inodes, true)
            .await?;
        self.update_identities(repo, &remote_changes).await?;

        self.complete_changes(repo, txn, local_channel, &pullable, false)
            .await?;
        Ok(())
    }
}

use libatomic::pristine::{ChangePosition, Position};
use regex::Regex;

lazy_static! {
    static ref CHANGELIST_LINE: Regex = Regex::new(
        r#"(?P<num>[0-9]+)\.(?P<hash>[A-Za-z0-9]+)\.(?P<merkle>[A-Za-z0-9]+)(?P<tag>\.)?"#
    )
    .unwrap();
    static ref PATHS_LINE: Regex =
        Regex::new(r#"(?P<hash>[A-Za-z0-9]+)\.(?P<num>[0-9]+)"#).unwrap();
}

enum ListLine {
    Change {
        n: u64,
        h: Hash,
        m: Merkle,
        tag: bool,
    },
    Position(Position<Hash>),
    Error(String),
}

fn parse_line(data: &str) -> Result<ListLine, anyhow::Error> {
    debug!("data = {:?}", data);
    if let Some(caps) = CHANGELIST_LINE.captures(data) {
        if let (Some(h), Some(m)) = (
            Hash::from_base32(caps.name("hash").unwrap().as_str().as_bytes()),
            Merkle::from_base32(caps.name("merkle").unwrap().as_str().as_bytes()),
        ) {
            return Ok(ListLine::Change {
                n: caps.name("num").unwrap().as_str().parse().unwrap(),
                h,
                m,
                tag: caps.name("tag").is_some(),
            });
        }
    }
    if data.starts_with("error:") {
        return Ok(ListLine::Error(data.split_at(6).1.to_string()));
    }
    if let Some(caps) = PATHS_LINE.captures(data) {
        return Ok(ListLine::Position(Position {
            change: Hash::from_base32(caps.name("hash").unwrap().as_str().as_bytes()).unwrap(),
            pos: ChangePosition(
                caps.name("num")
                    .unwrap()
                    .as_str()
                    .parse::<u64>()
                    .unwrap()
                    .into(),
            ),
        }));
    }
    debug!("offending line: {:?}", data);
    bail!("Protocol error")
}

/// Compare the remote set (theirs_ge_dichotomy) with our current
/// version of that (ours_ge_dichotomy) and return the changes in our
/// current version that are not in the remote anymore.
fn remote_unrecs<T: TxnTExt + ChannelTxnT>(
    txn: &T,
    current_channel: &ChannelRef<T>,
    ours_ge_dichotomy: &[(u64, Node)],
    theirs_ge_dichotomy_set: &HashSet<Node>,
) -> Result<Vec<(u64, Node)>, anyhow::Error> {
    let mut remote_unrecs = Vec::new();
    for (n, node) in ours_ge_dichotomy {
        debug!("ours_ge_dichotomy: {:?} {:?}", n, node);
        if theirs_ge_dichotomy_set.contains(node) {
            // If this change is still present in the remote, skip
            debug!("still present");
            continue;
        } else {
            let has_it = match node.node_type {
                NodeType::Change => txn.get_revchanges(&current_channel, &node.hash)?.is_some(),
                NodeType::Tag => {
                    let ch = current_channel.read();
                    let serialized_state: libatomic::pristine::SerializedMerkle =
                        (&node.state).into();
                    if let Some(n) = txn.channel_has_state(txn.states(&*ch), &serialized_state)? {
                        txn.is_tagged(txn.tags(&*ch), n.into())?
                    } else {
                        false
                    }
                }
            };
            if has_it {
                remote_unrecs.push((*n, *node))
            } else {
                // If this unrecord wasn't in our current channel, skip
                continue;
            }
        }
    }
    Ok(remote_unrecs)
}
