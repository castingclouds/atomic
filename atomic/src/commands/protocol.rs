use std::collections::{HashMap, HashSet};
use std::io::BufWriter;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use atomic_repository::Repository;
use byteorder::{BigEndian, WriteBytesExt};
use clap::Parser;
use lazy_static::lazy_static;
use libatomic::*;
use log::{debug, error, warn};
use regex::Regex;

/// This command is not meant to be run by the user,
/// instead it is called over SSH
#[derive(Parser, Debug)]
pub struct Protocol {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository")]
    repo_path: Option<PathBuf>,
    /// Use this protocol version
    #[clap(long = "version")]
    version: usize,
}

lazy_static! {
    static ref STATE: Regex = Regex::new(r#"state\s+(\S+)(\s+([0-9]+)?)\s+"#).unwrap();
    static ref ID: Regex = Regex::new(r#"id\s+(\S+)\s+"#).unwrap();
    static ref IDENTITIES: Regex = Regex::new(r#"identities(\s+([0-9]+))?\s+"#).unwrap();
    static ref CHANGELIST: Regex = Regex::new(r#"changelist\s+(\S+)\s+([0-9]+)(.*)\s+"#).unwrap();
    static ref CHANGELIST_PATHS: Regex = Regex::new(r#""(((\\")|[^"])+)""#).unwrap();
    static ref CHANGE: Regex = Regex::new(r#"((change)|(partial))\s+([^ ]*)\s+"#).unwrap();
    static ref TAG: Regex = Regex::new(r#"^tag\s+(\S+)\s+"#).unwrap();
    static ref TAGUP: Regex = Regex::new(r#"^tagup\s+(\S+)\s+(\S+)\s+([0-9]+)\s+"#).unwrap();
    static ref APPLY: Regex = Regex::new(r#"apply\s+(\S+)\s+([^ ]*) ([0-9]+)\s+"#).unwrap();
    static ref CHANNEL: Regex = Regex::new(r#"channel\s+(\S+)\s+"#).unwrap();
    static ref ARCHIVE: Regex =
        Regex::new(r#"archive\s+(\S+)\s*(( ([^:]+))*)( :(.*))?\n"#).unwrap();
}

fn load_channel<T: MutTxnTExt>(txn: &T, name: &str) -> Result<ChannelRef<T>, anyhow::Error> {
    if let Some(c) = txn.load_channel(name)? {
        Ok(c)
    } else {
        bail!("No such channel: {:?}", name)
    }
}

const PARTIAL_CHANGE_SIZE: u64 = 1 << 20;

impl Protocol {
    pub fn run(self) -> Result<(), anyhow::Error> {
        let mut repo = Repository::find_root(self.repo_path)?;
        let pristine = Arc::new(repo.pristine);
        let txn = pristine.arc_txn_begin()?;
        let mut ws = libatomic::ApplyWorkspace::new();
        let mut buf = String::new();
        let mut buf2 = vec![0; 4096 * 10];
        let s = std::io::stdin();
        let mut s = s.lock();
        let o = std::io::stdout();
        let mut o = BufWriter::new(o.lock());
        let mut applied = HashMap::new();

        debug!("reading");
        while s.read_line(&mut buf)? > 0 {
            debug!("{:?}", buf);
            if let Some(cap) = ID.captures(&buf) {
                let channel = load_channel(&*txn.read(), &cap[1])?;
                let c = channel.read();
                writeln!(o, "{}", c.id)?;
                o.flush()?;
            } else if let Some(cap) = STATE.captures(&buf) {
                let channel = load_channel(&*txn.read(), &cap[1])?;
                let init = if let Some(u) = cap.get(3) {
                    u.as_str().parse().ok()
                } else {
                    None
                };
                if let Some(pos) = init {
                    let txn = txn.read();
                    for x in txn.log(&*channel.read(), pos)? {
                        let (n, (_, m)) = x?;
                        match n.cmp(&pos) {
                            std::cmp::Ordering::Less => continue,
                            std::cmp::Ordering::Greater => {
                                writeln!(o, "-")?;
                                break;
                            }
                            std::cmp::Ordering::Equal => {
                                let m: libatomic::Merkle = m.into();
                                let m2 = if let Some(x) = txn
                                    .rev_iter_tags(txn.tags(&*channel.read()), Some(n))?
                                    .next()
                                {
                                    let tag_bytes = x?.1;
                                    let serialized =
                                        libatomic::pristine::SerializedTag::from_bytes_wrapper(
                                            tag_bytes,
                                        );
                                    if let Ok(tag) = serialized.to_tag() {
                                        tag.state
                                    } else {
                                        Merkle::zero()
                                    }
                                } else {
                                    Merkle::zero()
                                };
                                writeln!(o, "{} {} {}", n, m.to_base32(), m2.to_base32())?;
                                break;
                            }
                        }
                    }
                } else {
                    let txn = txn.read();
                    if let Some(x) = txn.reverse_log(&*channel.read(), None)?.next() {
                        let (n, (_, m)) = x?;
                        let m: Merkle = m.into();
                        let m2 = if let Some(x) = txn
                            .rev_iter_tags(txn.tags(&*channel.read()), Some(n))?
                            .next()
                        {
                            let tag_bytes = x?.1;
                            let serialized =
                                libatomic::pristine::SerializedTag::from_bytes_wrapper(tag_bytes);
                            if let Ok(tag) = serialized.to_tag() {
                                tag.state
                            } else {
                                Merkle::zero()
                            }
                        } else {
                            Merkle::zero()
                        };
                        writeln!(o, "{} {} {}", n, m.to_base32(), m2.to_base32())?
                    } else {
                        writeln!(o, "-")?;
                    }
                }
                o.flush()?;
            } else if let Some(cap) = CHANGELIST.captures(&buf) {
                let channel = load_channel(&*txn.read(), &cap[1])?;
                let from: u64 = cap[2].parse().unwrap();
                let mut paths = Vec::new();
                let txn = txn.read();
                {
                    for r in CHANGELIST_PATHS.captures_iter(&cap[3]) {
                        let s: String = r[1].replace("\\\"", "\"");
                        if let Ok((p, ambiguous)) =
                            txn.follow_oldest_path(&repo.changes, &channel, &s)
                        {
                            if ambiguous {
                                bail!("Ambiguous path")
                            }
                            let h: libatomic::Hash = txn.get_external(&p.change)?.unwrap().into();
                            writeln!(o, "{}.{}", h.to_base32(), p.pos.0)?;
                            paths.push(s);
                        } else {
                            debug!("protocol line: {:?}", buf);
                            bail!("Protocol error")
                        }
                    }
                }
                let mut tagsi = 0;
                (atomic_remote::local::Local {
                    channel: (&cap[1]).to_string(),
                    root: PathBuf::new(),
                    changes_dir: PathBuf::new(),
                    pristine: pristine.clone(),
                    name: String::new(),
                })
                .download_changelist_(
                    |_, n, h, m, is_tag| {
                        if is_tag {
                            writeln!(o, "{}.{}.{}.", n, h.to_base32(), m.to_base32())?;
                            tagsi += 1;
                        } else {
                            writeln!(o, "{}.{}.{}", n, h.to_base32(), m.to_base32())?;
                        }
                        Ok(())
                    },
                    &mut (),
                    from,
                    &paths,
                    &*txn,
                    &channel,
                )?;
                writeln!(o)?;
                o.flush()?;
            } else if let Some(cap) = TAG.captures(&buf) {
                if let Some(state) = Merkle::from_base32(cap[1].as_bytes()) {
                    let mut tag_path = repo.changes_dir.clone();
                    libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &state);
                    let mut tag = libatomic::tag::OpenTagFile::open(&tag_path, &state)?;
                    let mut buf = Vec::new();
                    tag.short(&mut buf)?;
                    o.write_u64::<BigEndian>(buf.len() as u64)?;
                    o.write_all(&buf)?;
                    o.flush()?;
                }
            } else if let Some(cap) = TAGUP.captures(&buf) {
                if let Some(state) = Merkle::from_base32(cap[1].as_bytes()) {
                    let channel = load_channel(&*txn.read(), &cap[2])?;
                    let m = libatomic::pristine::current_state(&*txn.read(), &*channel.read())?;
                    if m == state {
                        let mut tag_path = repo.changes_dir.clone();
                        libatomic::changestore::filesystem::push_tag_filename(&mut tag_path, &m);
                        if std::fs::metadata(&tag_path).is_ok() {
                            bail!("Tag for state {} already exists", m.to_base32());
                        }

                        let last_t = if let Some(n) =
                            txn.read().reverse_log(&*channel.read(), None)?.next()
                        {
                            n?.0.into()
                        } else {
                            bail!("Channel {} is empty", &cap[2]);
                        };
                        if txn.read().is_tagged(&channel.read().tags, last_t)? {
                            bail!("Current state is already tagged")
                        }

                        let size: usize = cap[3].parse().unwrap();
                        let mut buf = vec![0; size];
                        s.read_exact(&mut buf)?;

                        let header =
                            libatomic::tag::read_short(std::io::Cursor::new(&buf[..]), &m)?;

                        let temp_path = tag_path.with_extension("tmp");

                        std::fs::create_dir_all(temp_path.parent().unwrap())?;
                        let mut w = std::fs::File::create(&temp_path)?;
                        libatomic::tag::from_channel(&*txn.read(), &cap[2], &header, &mut w)?;

                        std::fs::rename(&temp_path, &tag_path)?;

                        // Store consolidating tag metadata (matching HTTP API behavior)
                        {
                            use libatomic::pristine::{SerializedTag, Tag, TagMetadataMutTxnT};

                            let channel_name = &cap[2];

                            // Calculate consolidating tag metadata
                            let (_start_position, consolidated_changes, change_count) = {
                                let channel_read = channel.read();
                                let txn_read = txn.read();

                                // Find starting position
                                let mut last_tag_pos = None;
                                if let Ok(iter) =
                                    txn_read.rev_iter_tags(txn_read.tags(&*channel_read), None)
                                {
                                    for entry in iter {
                                        if let Ok((pos, _tag_bytes)) = entry {
                                            last_tag_pos = Some(pos);
                                            break;
                                        }
                                    }
                                }
                                let start_pos = last_tag_pos.map(|p| p.0 + 1).unwrap_or(0);

                                // Collect changes
                                let mut changes = Vec::new();
                                let mut count = 0u64;
                                if let Ok(log_iter) = txn_read.log(&*channel_read, start_pos) {
                                    for entry in log_iter {
                                        if let Ok((pos, (hash, _))) = entry {
                                            let hash: libatomic::pristine::Hash = hash.into();
                                            debug!(
                                                "  Position {}: including change {}",
                                                pos,
                                                hash.to_base32()
                                            );
                                            changes.push(hash);
                                            count += 1;
                                        }
                                    }
                                }

                                (start_pos, changes, count)
                            };

                            let dependency_count_before = change_count;
                            let consolidated_change_count = change_count;
                            let original_timestamp = header.timestamp.timestamp() as u64;

                            // Create consolidating tag metadata
                            let tag_hash = m;
                            let mut tag = Tag::new(
                                tag_hash,
                                m.clone(),
                                channel_name.to_string(),
                                None,
                                dependency_count_before,
                                consolidated_change_count,
                                consolidated_changes,
                            );

                            // Use the original timestamp from the tag header
                            tag.consolidation_timestamp = original_timestamp;
                            tag.change_file_hash = Some(m);

                            // Serialize and store consolidating tag metadata
                            let serialized = SerializedTag::from_tag(&tag)?;
                            txn.write().put_tag(&tag_hash, &serialized)?;

                            debug!(
                                "Stored consolidating tag metadata for {}",
                                tag_hash.to_base32()
                            );
                        }

                        txn.write()
                            .put_tags(&mut channel.write().tags, last_t.into(), &m)?;
                    } else {
                        bail!("Wrong state, cannot tag")
                    }
                }
            } else if let Some(cap) = CHANGE.captures(&buf) {
                let h_ = &cap[4];
                let h = if let Some(h) = Hash::from_base32(h_.as_bytes()) {
                    h
                } else {
                    debug!("protocol error: {:?}", buf);
                    bail!("Protocol error")
                };
                libatomic::changestore::filesystem::push_filename(&mut repo.changes_dir, &h);
                debug!("repo = {:?}", repo.changes_dir);
                let mut f = std::fs::File::open(&repo.changes_dir)?;
                let size = std::fs::metadata(&repo.changes_dir)?.len();
                let size = if &cap[1] == "change" || size <= PARTIAL_CHANGE_SIZE {
                    size
                } else {
                    libatomic::change::Change::size_no_contents(&mut f)?
                };
                o.write_u64::<BigEndian>(size)?;
                let mut size = size as usize;
                while size > 0 {
                    if size < buf2.len() {
                        buf2.truncate(size as usize);
                    }
                    let n = f.read(&mut buf2[..])?;
                    if n == 0 {
                        break;
                    }
                    size -= n;
                    o.write_all(&buf2[..n])?;
                }
                o.flush()?;
                libatomic::changestore::filesystem::pop_filename(&mut repo.changes_dir);
            } else if let Some(cap) = APPLY.captures(&buf) {
                let h = if let Some(h) = Hash::from_base32(cap[2].as_bytes()) {
                    h
                } else {
                    debug!("protocol error {:?}", buf);
                    bail!("Protocol error");
                };
                let mut path = repo.changes_dir.clone();
                libatomic::changestore::filesystem::push_filename(&mut path, &h);
                std::fs::create_dir_all(path.parent().unwrap())?;
                let size: usize = cap[3].parse().unwrap();
                buf2.resize(size, 0);
                s.read_exact(&mut buf2)?;
                std::fs::write(&path, &buf2)?;
                libatomic::change::Change::deserialize(&path.to_string_lossy(), Some(&h))?;
                let channel = load_channel(&*txn.read(), &cap[1])?;
                {
                    let mut channel_ = channel.write();
                    txn.write().apply_node_ws(
                        &repo.changes,
                        &mut channel_,
                        &h,
                        libatomic::pristine::NodeType::Change,
                        &mut ws,
                    )?;
                }
                applied.insert(cap[1].to_string(), channel);
            } else if let Some(cap) = ARCHIVE.captures(&buf) {
                let mut w = Vec::new();
                let mut tarball = libatomic::output::Tarball::new(
                    &mut w,
                    cap.get(6).map(|x| x.as_str().to_string()),
                    0,
                );
                let channel = load_channel(&*txn.read(), &cap[1])?;
                let conflicts = if let Some(caps) = cap.get(2) {
                    debug!("caps = {:?}", caps.as_str());
                    let mut hashes = caps.as_str().split(' ').filter(|x| !x.is_empty());
                    let state: libatomic::Merkle = hashes.next().unwrap().parse().unwrap();
                    let extra: Vec<libatomic::Hash> = hashes.map(|x| x.parse().unwrap()).collect();
                    debug!("state = {:?}, extra = {:?}", state, extra);
                    if txn.read().current_state(&*channel.read())? == state && extra.is_empty() {
                        txn.archive(&repo.changes, &channel, &mut tarball)?
                    } else {
                        use rand::Rng;
                        let fork_name: String = rand::thread_rng()
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(30)
                            .map(|x| x as char)
                            .collect();
                        let mut fork = {
                            let mut txn = txn.write();
                            txn.fork(&channel, &fork_name)?
                        };
                        let conflicts = txn.archive_with_state(
                            &repo.changes,
                            &mut fork,
                            &state,
                            &extra,
                            &mut tarball,
                            0,
                        )?;
                        txn.write().drop_channel(&fork_name)?;
                        conflicts
                    }
                } else {
                    txn.archive(&repo.changes, &channel, &mut tarball)?
                };
                std::mem::drop(tarball);
                let mut o = std::io::stdout();
                o.write_u64::<BigEndian>(w.len() as u64)?;
                o.write_u64::<BigEndian>(conflicts.len() as u64)?;
                o.write_all(&w)?;
                o.flush()?;
            } else if let Some(cap) = IDENTITIES.captures(&buf) {
                let last_touched: u64 = if let Some(last) = cap.get(2) {
                    last.as_str().parse().unwrap()
                } else {
                    0
                };
                let mut id_dir = repo.path.clone();
                id_dir.push(DOT_DIR);
                id_dir.push("identities");
                let r = if let Ok(r) = std::fs::read_dir(&id_dir) {
                    r
                } else {
                    writeln!(o)?;
                    o.flush()?;
                    continue;
                };
                let mut at_least_one = false;
                for id in r {
                    at_least_one |= output_id(id, last_touched, &mut o).unwrap_or(false);
                }
                debug!("at least one {:?}", at_least_one);
                if !at_least_one {
                    writeln!(o)?;
                }
                writeln!(o)?;
                o.flush()?;
            } else {
                error!("unmatched")
            }
            buf.clear();
        }
        let applied_nonempty = !applied.is_empty();
        for (_, channel) in applied {
            libatomic::output::output_repository_no_pending(
                &repo.working_copy,
                &repo.changes,
                &txn,
                &channel,
                "",
                true,
                None,
                std::thread::available_parallelism()?.get(),
                0,
            )?;
        }
        if applied_nonempty {
            txn.commit()?;
        }
        Ok(())
    }
}

fn output_id<W: Write>(
    id: Result<std::fs::DirEntry, std::io::Error>,
    last_touched: u64,
    mut o: W,
) -> Result<bool, anyhow::Error> {
    let id = id?;
    let m = id.metadata()?;
    let p = id.path();
    debug!("{:?}", p);
    let mod_ts = m
        .modified()?
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    if mod_ts >= last_touched {
        let mut done = HashSet::new();
        if p.file_name() == Some("publickey.json".as_ref()) {
            warn!("Skipping serializing old public key format.");
            return Ok(false);
        } else {
            let mut idf = if let Ok(f) = std::fs::File::open(&p) {
                f
            } else {
                return Ok(false);
            };
            let id: Result<atomic_identity::Complete, _> = serde_json::from_reader(&mut idf);
            if let Ok(id) = id {
                if !done.insert(id.public_key.key.clone()) {
                    return Ok(false);
                }
                serde_json::to_writer(&mut o, &id.as_portable()).unwrap();
                writeln!(o)?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}
