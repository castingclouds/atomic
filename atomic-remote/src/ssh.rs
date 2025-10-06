use std::collections::HashSet;
use std::convert::TryInto;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::bail;
use byteorder::{BigEndian, ReadBytesExt};
use lazy_static::lazy_static;
use libatomic::pristine::Position;
use libatomic::{Base32, Hash, Merkle};
use log::{debug, error, info, trace, warn};
use regex::Regex;
use thrussh::client::Session;
use tokio::sync::Mutex;

use super::parse_line;
use crate::Node;
use atomic_interaction::ProgressBar;
use libatomic::pristine::NodeType;

pub struct Ssh {
    pub h: thrussh::client::Handle<SshClient>,
    pub c: thrussh::client::Channel,
    pub channel: String,
    pub remote_cmd: String,
    pub path: String,
    pub is_running: bool,
    pub name: String,
    state: Arc<Mutex<State>>,
    has_errors: Arc<Mutex<bool>>,
}

lazy_static! {
    static ref ADDRESS: Regex = Regex::new(
        r#"(ssh://)?((?P<user>[^@]+)@)?((?P<host>(\[([^\]]+)\])|([^:/]+)))((:(?P<port>\d+)(?P<path0>(/.+)))|(:(?P<path1>.+))|(?P<path2>(/.+)))"#
    )
        .unwrap();

    static ref ADDRESS_NOPATH: Regex = Regex::new(
        r#"(ssh://)?((?P<user>[^@]+)@)?((?P<host>(\[([^\]]+)\])|([^:/]+)))(:(?P<port>\d+))?"#
    )
        .unwrap();
}

#[derive(Debug)]
pub struct Remote<'a> {
    path: &'a str,
    config: thrussh_config::Config,
}

pub fn ssh_remote<'a>(user: Option<&str>, addr: &'a str, with_path: bool) -> Option<Remote<'a>> {
    let cap = if with_path {
        ADDRESS.captures(addr)?
    } else {
        ADDRESS_NOPATH.captures(addr)?
    };
    debug!("ssh_remote: {:?}", cap);
    let host = cap.name("host").unwrap().as_str();

    let mut config =
        thrussh_config::parse_home(&host).unwrap_or(thrussh_config::Config::default(host));
    if let Some(port) = cap.name("port").map(|x| x.as_str().parse().unwrap()) {
        config.port = port
    }
    if let Some(u) = cap.name("user") {
        config.user.clear();
        config.user.push_str(u.as_str());
    }
    if let Some(user) = user {
        if !user.is_empty() {
            config.user.clear();
            config.user.push_str(user)
        }
    }
    let path = if with_path {
        let p = cap
            .name("path0")
            .unwrap_or_else(|| {
                cap.name("path1")
                    .unwrap_or_else(|| cap.name("path2").unwrap())
            })
            .as_str();
        if p.starts_with("/~") {
            p.split_at(1).1
        } else {
            p
        }
    } else {
        ""
    };
    Some(Remote { path, config })
}

impl<'a> Remote<'a> {
    pub async fn connect(
        &mut self,
        name: &str,
        channel: &str,
    ) -> Result<Option<Ssh>, anyhow::Error> {
        let mut home = dirs_next::home_dir().unwrap();
        home.push(".ssh");
        home.push("known_hosts");
        let state = Arc::new(Mutex::new(State::None));
        let has_errors = Arc::new(Mutex::new(false));
        let client = SshClient {
            addr: self.config.host_name.clone(),
            port: self.config.port,
            known_hosts: home,
            last_window_adjustment: SystemTime::now(),
            state: state.clone(),
            has_errors: has_errors.clone(),
        };
        let stream = match self.config.stream().await {
            Ok(stream) => stream,
            Err(e) => {
                info!("remote connect error: {:?}", e);
                return Ok(None);
            }
        };
        let config = Arc::new(thrussh::client::Config::default());
        let mut h = thrussh::client::connect_stream(config, stream, client).await?;

        let mut key_path = dirs_next::home_dir().unwrap().join(".ssh");

        // First try agent auth
        let authenticated = match self.auth_agent(&mut h, &mut key_path).await {
            Ok(true) => true,
            Ok(false) => {
                if self.auth_pk(&mut h, &mut key_path).await? {
                    true
                } else {
                    let mut stderr = std::io::stderr();
                    writeln!(stderr, "Warning: Unable to automatically authenticate with server. Please make sure your SSH keys have been uploaded to the Nest.")?;
                    writeln!(stderr, "For more information, please visit https://atomic.org/manual/the_nest/public_keys.html#ssh-public-keys")?;
                    self.auth_password(&mut h).await?
                }
            }
            Err(e) => return Err(e.into()),
        };

        if !authenticated {
            bail!("Not authenticated. Please check your credentials and try again.");
        }

        let c = h.channel_open_session().await?;
        let remote_cmd = if let Ok(cmd) = std::env::var("REMOTE_atomic") {
            cmd
        } else {
            "atomic".to_string()
        };
        Ok(Some(Ssh {
            h,
            c,
            channel: channel.to_string(),
            remote_cmd,
            path: self.path.to_string(),
            is_running: false,
            name: name.to_string(),
            state,
            has_errors,
        }))
    }

    async fn auth_agent(
        &self,
        h: &mut thrussh::client::Handle<SshClient>,
        key_path: &mut PathBuf,
    ) -> Result<bool, thrussh::Error> {
        let mut authenticated = false;
        let mut agent = match thrussh_keys::agent::client::AgentClient::connect_env().await {
            Ok(agent) => agent,
            Err(thrussh_keys::Error::EnvVar(_)) => return Ok(false),
            Err(thrussh_keys::Error::AgentFailure) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let identities = if let Some(ref file) = self.config.identity_file {
            key_path.push(file);
            key_path.set_extension("pub");
            let k = thrussh_keys::load_public_key(&key_path);
            key_path.pop();
            if let Ok(k) = k {
                vec![k]
            } else {
                return Ok(false);
            }
        } else {
            agent.request_identities().await?
        };
        debug!("identities = {:?}", identities);
        let mut agent = Some(agent);
        for key in identities {
            debug!("Trying key {:?}", key);
            debug!("fingerprint = {:?}", key.fingerprint());
            if let Some(a) = agent.take() {
                debug!("authenticate future");
                match h.authenticate_future(&self.config.user, key, a).await {
                    (a, Ok(auth)) => {
                        authenticated = auth;
                        agent = Some(a);
                    }
                    (_, Err(thrussh::AgentAuthError::Send(e))) => {
                        debug!("send error {:?}", e);
                        return Err(thrussh::Error::SendError);
                    }
                    (a, Err(e)) => {
                        agent = Some(a);
                        debug!("not auth {:?}", e);
                        if let thrussh::AgentAuthError::Key(e) = e {
                            debug!("error: {:?}", e);
                            writeln!(std::io::stderr(), "Failed to sign with agent")?;
                        }
                    }
                }
            }
            if authenticated {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn auth_pk(
        &self,
        h: &mut thrussh::client::Handle<SshClient>,
        key_path: &mut PathBuf,
    ) -> Result<bool, anyhow::Error> {
        if h.is_closed() {
            return Ok(false);
        }
        let mut authenticated = false;
        let mut keys = Vec::new();
        if let Some(ref file) = self.config.identity_file {
            keys.push(file.as_str())
        } else {
            keys.push("id_ed25519");
            keys.push("id_rsa");
        }
        for k in keys.iter() {
            key_path.push(k);
            let k = match thrussh_keys::load_secret_key(&key_path, None) {
                Ok(k) => k,
                Err(thrussh_keys::Error::KeyIsEncrypted) => {
                    let password = atomic_interaction::Password::new()?
                        .with_prompt(format!("Password for encrypted private key"))
                        .with_allow_empty(false)
                        .interact()?;
                    if let Ok(k) = thrussh_keys::load_secret_key(&key_path, Some(&password)) {
                        k
                    } else {
                        continue;
                    }
                }
                Err(_) => {
                    key_path.pop();
                    continue;
                }
            };
            if let Ok(auth) = h
                .authenticate_publickey(&self.config.user, Arc::new(k))
                .await
            {
                authenticated = auth
            }
            key_path.pop();
            if authenticated {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn auth_password(
        &self,
        h: &mut thrussh::client::Handle<SshClient>,
    ) -> Result<bool, anyhow::Error> {
        if h.is_closed() {
            return Ok(false);
        }

        // Authentication can be attempted multiple times
        let mut authenticated = false;
        let username = format!("{}@{}", self.config.user, self.config.host_name);

        // Try authenticate using the user's keyring
        if let Ok(password) =
            keyring::Entry::new("atomic", &username).and_then(|x| x.get_password())
        {
            authenticated = h
                .authenticate_password(self.config.user.to_string(), &password)
                .await?;
        }

        // Try authenticate using user's password
        if !authenticated {
            let password = atomic_interaction::Password::new()?
                .with_prompt(format!("Password for {username}"))
                .with_allow_empty(true)
                .interact()?;

            authenticated = h
                .authenticate_password(self.config.user.to_string(), &password)
                .await?;

            // If the new password is valid, update the keyring to match
            if authenticated {
                if let Err(e) =
                    keyring::Entry::new("atomic", &username).and_then(|x| x.set_password(&password))
                {
                    warn!("Unable to set password: {e:?}");
                }
            }
        }

        Ok(authenticated)
    }
}

pub struct SshClient {
    addr: String,
    port: u16,
    known_hosts: PathBuf,
    last_window_adjustment: SystemTime,
    state: Arc<Mutex<State>>,
    has_errors: Arc<Mutex<bool>>,
}

enum State {
    None,
    State {
        sender: Option<tokio::sync::oneshot::Sender<Option<(u64, Merkle, Merkle)>>>,
    },
    Id {
        sender: Option<tokio::sync::oneshot::Sender<Option<libatomic::pristine::RemoteId>>>,
    },
    Changes {
        sender: Option<tokio::sync::mpsc::Sender<Node>>,
        remaining_len: usize,
        file: std::fs::File,
        path: PathBuf,
        final_path: PathBuf,
        hashes: Vec<Node>,
        current: usize,
    },
    Changelist {
        sender: tokio::sync::mpsc::Sender<Option<super::ListLine>>,
        pending: Vec<u8>,
    },
    Archive {
        sender: Option<tokio::sync::oneshot::Sender<u64>>,
        len: u64,
        conflicts: u64,
        len_n: u64,
        w: Box<dyn Write + Send>,
    },
    Prove {
        key: libatomic::key::SKey,
        sender: Option<tokio::sync::oneshot::Sender<()>>,
        signed: bool,
    },
    Identities {
        sender: Option<tokio::sync::mpsc::Sender<atomic_identity::Complete>>,
        buf: Vec<u8>,
    },
}

type BoxFuture<T> = Pin<Box<dyn futures::future::Future<Output = T> + Send>>;

impl thrussh::client::Handler for SshClient {
    type Error = anyhow::Error;
    type FutureBool = futures::future::Ready<Result<(Self, bool), anyhow::Error>>;
    type FutureUnit = BoxFuture<Result<(Self, Session), anyhow::Error>>;

    fn finished_bool(self, b: bool) -> Self::FutureBool {
        futures::future::ready(Ok((self, b)))
    }
    fn finished(self, session: Session) -> Self::FutureUnit {
        Box::pin(async move { Ok((self, session)) })
    }
    fn check_server_key(
        self,
        server_public_key: &thrussh_keys::key::PublicKey,
    ) -> Self::FutureBool {
        debug!("addr = {:?} port = {:?}", self.addr, self.port);
        match thrussh_keys::check_known_hosts_path(
            &self.addr,
            self.port,
            server_public_key,
            &self.known_hosts,
        ) {
            Ok(e) => {
                if e {
                    futures::future::ready(Ok((self, true)))
                } else {
                    match learn(&self.addr, self.port, server_public_key) {
                        Ok(x) => futures::future::ready(Ok((self, x))),
                        Err(e) => futures::future::ready(Err(e)),
                    }
                }
            }
            Err(e) => {
                writeln!(std::io::stderr(), "Key changed for {:?}", self.addr).unwrap_or(());

                futures::future::ready(Err(e.into()))
            }
        }
    }

    fn adjust_window(&mut self, _channel: thrussh::ChannelId, target: u32) -> u32 {
        let elapsed = self.last_window_adjustment.elapsed().unwrap();
        self.last_window_adjustment = SystemTime::now();
        if target >= 10_000_000 {
            return target;
        }
        if elapsed < Duration::from_secs(2) {
            target * 2
        } else if elapsed > Duration::from_secs(8) {
            target / 2
        } else {
            target
        }
    }

    fn channel_eof(
        self,
        _channel: thrussh::ChannelId,
        session: thrussh::client::Session,
    ) -> Self::FutureUnit {
        Box::pin(async move {
            *self.state.lock().await = State::None;
            Ok((self, session))
        })
    }

    fn exit_status(
        self,
        channel: thrussh::ChannelId,
        exit_status: u32,
        session: thrussh::client::Session,
    ) -> Self::FutureUnit {
        session.send_channel_msg(channel, thrussh::ChannelMsg::ExitStatus { exit_status });
        Box::pin(async move {
            *self.state.lock().await = State::None;
            *self.has_errors.lock().await = true;
            Ok((self, session))
        })
    }

    fn extended_data(
        self,
        channel: thrussh::ChannelId,
        ext: u32,
        data: &[u8],
        session: thrussh::client::Session,
    ) -> Self::FutureUnit {
        debug!("extended data {:?}, {:?}", std::str::from_utf8(data), ext);
        if ext == 0 {
            self.data(channel, data, session)
        } else {
            let data = data.to_vec();
            Box::pin(async move {
                *self.has_errors.lock().await = true;
                let stderr = std::io::stderr();
                let mut handle = stderr.lock();
                handle.write_all(&data)?;
                Ok((self, session))
            })
        }
    }

    fn data(
        self,
        channel: thrussh::ChannelId,
        data: &[u8],
        mut session: thrussh::client::Session,
    ) -> Self::FutureUnit {
        trace!("data {:?} {:?}", channel, data.len());
        let data = data.to_vec();
        Box::pin(async move {
            match *self.state.lock().await {
                State::State { ref mut sender } => {
                    debug!("state: State");
                    if let Some(sender) = sender.take() {
                        // If we can't parse `data` (for example if the
                        // remote returns the standard "-\n"), this
                        // returns None.
                        let mut s = std::str::from_utf8(&data).unwrap().split(' ');
                        if let (Some(n), Some(m), Some(m2)) = (s.next(), s.next(), s.next()) {
                            let n = n.parse().unwrap();
                            sender
                                .send(Some((
                                    n,
                                    Merkle::from_base32(m.trim().as_bytes()).unwrap(),
                                    Merkle::from_base32(m2.trim().as_bytes()).unwrap(),
                                )))
                                .unwrap_or(());
                        } else {
                            sender.send(None).unwrap_or(());
                        }
                    }
                }
                State::Id { ref mut sender } => {
                    debug!("state: Id {:?}", std::str::from_utf8(&data));
                    if let Some(sender) = sender.take() {
                        let line = if data.len() >= 16 && data.last() == Some(&10) {
                            libatomic::pristine::RemoteId::from_base32(&data[..data.len() - 1])
                        } else {
                            None
                        };
                        if let Some(b) = line {
                            sender.send(Some(b)).unwrap_or(());
                        } else {
                            sender.send(None).unwrap_or(());
                        }
                    }
                }
                State::Changes {
                    ref mut sender,
                    ref mut remaining_len,
                    ref mut file,
                    ref mut path,
                    ref mut final_path,
                    ref hashes,
                    ref mut current,
                } => {
                    trace!("state changes");
                    let mut p = 0;
                    while p < data.len() {
                        if *remaining_len == 0 {
                            *remaining_len = (&data[p..]).read_u64::<BigEndian>().unwrap() as usize;
                            p += 8;
                            debug!("remaining_len = {:?}", remaining_len);
                        }
                        if data.len() >= p + *remaining_len {
                            debug!("writing {:?} bytes", *remaining_len);
                            file.write_all(&data[p..p + *remaining_len])?;
                            // We have enough data to write the
                            // file, write it and move to the next
                            // file.
                            p += *remaining_len;
                            *remaining_len = 0;
                            file.flush()?;

                            match hashes[*current].node_type {
                                NodeType::Change => {
                                    libatomic::changestore::filesystem::push_filename(
                                        final_path,
                                        &hashes[*current].hash,
                                    );
                                    debug!("moving {:?} to {:?}", path, final_path);
                                    std::fs::create_dir_all(&final_path.parent().unwrap())?;
                                    let r = std::fs::rename(&path, &final_path);
                                    libatomic::changestore::filesystem::pop_filename(final_path);
                                    r?;
                                }
                                NodeType::Tag => {
                                    libatomic::changestore::filesystem::push_tag_filename(
                                        final_path,
                                        &hashes[*current].state,
                                    );
                                    debug!("moving {:?} to {:?}", path, final_path);
                                    std::fs::create_dir_all(&final_path.parent().unwrap())?;
                                    let r = std::fs::rename(&path, &final_path);
                                    libatomic::changestore::filesystem::pop_filename(final_path);
                                    r?;
                                }
                            }
                            debug!("sending {:?}", hashes[*current]);
                            if let Some(ref mut sender) = sender {
                                if sender.send(hashes[*current]).await.is_err() {
                                    break;
                                }
                            }
                            debug!("sent");
                            *current += 1;
                            if *current < hashes.len() {
                                // If we're still waiting for another
                                // change.
                                *file = std::fs::File::create(&path)?;
                            } else {
                                // Else, just finish.
                                debug!("dropping channel");
                                std::mem::drop(sender.take());
                                break;
                            }
                        } else {
                            // not enough data, we need more.
                            trace!(
                                "writing to {:?} {:?} {:?}",
                                path,
                                final_path,
                                hashes[*current]
                            );

                            file.write_all(&data[p..])?;
                            file.flush()?;
                            *remaining_len -= data.len() - p;
                            trace!("need more data");
                            break;
                        }
                    }
                    trace!("finished, {:?} {:?}", p, data.len());
                }
                State::Changelist {
                    ref mut sender,
                    ref mut pending,
                } => {
                    debug!("state changelist");
                    if &data[..] == b"\n" {
                        debug!("log done");
                        sender.send(None).await.unwrap_or(())
                    } else {
                        trace!("{:?}", data);
                        let mut p = 0;
                        while let Some(i) = (&data[p..]).iter().position(|i| *i == b'\n') {
                            let line = if !pending.is_empty() {
                                pending.extend(&data[p..p + i]);
                                &pending
                            } else {
                                &data[p..p + i]
                            };
                            let l = std::str::from_utf8(line)?;
                            if !l.is_empty() {
                                debug!("line = {:?}", l);
                                sender.send(parse_line(l).ok()).await.unwrap_or(())
                            } else {
                                sender.send(None).await.unwrap_or(());
                            }
                            pending.clear();
                            p += i + 1;
                        }
                        pending.extend(&data[p..]);
                    }
                }
                State::Archive {
                    ref mut sender,
                    ref mut w,
                    ref mut len,
                    ref mut len_n,
                    ref mut conflicts,
                } => {
                    debug!("state archive");
                    let mut off = 0;
                    while *len_n < 16 && off < data.len() {
                        if *len_n < 8 {
                            *len = (*len << 8) | (data[off] as u64);
                        } else {
                            *conflicts = (*conflicts << 8) | (data[off] as u64);
                        }
                        *len_n += 1;
                        off += 1;
                    }
                    if *len_n >= 16 {
                        w.write_all(&data[off..])?;
                        *len -= (data.len() - off) as u64;
                        if *len == 0 {
                            if let Some(sender) = sender.take() {
                                sender.send(*conflicts).unwrap_or(())
                            }
                        }
                    }
                }
                State::Prove {
                    ref mut key,
                    ref mut sender,
                    ref mut signed,
                } => {
                    if let Ok(data) = std::str::from_utf8(&data) {
                        if *signed && !data.trim().is_empty() {
                            std::io::stderr().write_all(data.as_bytes())?;
                        } else {
                            let data = data.trim();
                            debug!("signing {:?}", data);
                            let s = key.sign_raw(data.as_bytes())?;
                            session.data(
                                channel,
                                thrussh::CryptoVec::from_slice(format!("prove {}\n", s).as_bytes()),
                            );
                            if let Some(sender) = sender.take() {
                                sender.send(()).unwrap_or(());
                            }
                            *signed = true;
                        }
                    }
                }
                State::Identities {
                    ref mut sender,
                    ref mut buf,
                } => {
                    debug!("data = {:?}", data);
                    if data.ends_with(&[10]) {
                        let buf_ = if buf.is_empty() {
                            &data
                        } else {
                            buf.extend(&data);
                            &buf
                        };
                        for data in buf_.split(|c| *c == 10) {
                            if let Ok(p) = serde_json::from_slice(data) {
                                debug!("p = {:?}", p);
                                if let Some(ref mut sender) = sender {
                                    sender.send(p).await?;
                                }
                            } else {
                                debug!("could not parse {:?}", std::str::from_utf8(&data));
                                *sender = None;
                                break;
                            }
                        }
                        buf.clear()
                    } else {
                        buf.extend(&data);
                    }
                }
                State::None => {
                    debug!("None state");
                }
            }
            Ok((self, session))
        })
    }
}

fn learn(addr: &str, port: u16, pk: &thrussh_keys::key::PublicKey) -> Result<bool, anyhow::Error> {
    if port == 22 {
        print!(
            "Unknown key for {:?}, fingerprint {:?}. Learn it (y/N)? ",
            addr,
            pk.fingerprint()
        );
    } else {
        print!(
            "Unknown key for {:?}:{}, fingerprint {:?}. Learn it (y/N)? ",
            addr,
            port,
            pk.fingerprint()
        );
    }
    std::io::stdout().flush()?;
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;
    let buffer = buffer.trim();
    if buffer == "Y" || buffer == "y" {
        thrussh_keys::learn_known_hosts(addr, port, pk)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

impl Ssh {
    pub async fn finish(&mut self) -> Result<(), anyhow::Error> {
        self.c.eof().await?;
        while let Some(msg) = self.c.wait().await {
            debug!("msg = {:?}", msg);
            match msg {
                thrussh::ChannelMsg::WindowAdjusted { .. } => {}
                thrussh::ChannelMsg::Eof => {}
                thrussh::ChannelMsg::ExitStatus { exit_status } => {
                    if exit_status != 0 {
                        bail!("Remote exited with status {:?}", exit_status)
                    }
                }
                msg => error!("wrong message {:?}", msg),
            }
        }
        Ok(())
    }

    pub async fn get_state(
        &mut self,
        mid: Option<u64>,
    ) -> Result<Option<(u64, Merkle, Merkle)>, anyhow::Error> {
        debug!("get_state");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        *self.state.lock().await = State::State {
            sender: Some(sender),
        };
        self.run_protocol().await?;
        if let Some(mid) = mid {
            self.c
                .data(format!("state {} {}\n", self.channel, mid).as_bytes())
                .await?;
        } else {
            self.c
                .data(format!("state {}\n", self.channel).as_bytes())
                .await?;
        }
        Ok(receiver.await?)
    }

    pub async fn get_id(&mut self) -> Result<Option<libatomic::pristine::RemoteId>, anyhow::Error> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        *self.state.lock().await = State::Id {
            sender: Some(sender),
        };
        self.run_protocol().await?;
        self.c
            .data(format!("id {}\n", self.channel).as_bytes())
            .await?;
        Ok(receiver.await?)
    }

    pub async fn prove(&mut self, key: libatomic::key::SKey) -> Result<(), anyhow::Error> {
        debug!("get_state");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let k = serde_json::to_string(&key.public_key())?;
        *self.state.lock().await = State::Prove {
            key,
            sender: Some(sender),
            signed: false,
        };
        self.run_protocol().await?;
        self.c.data(format!("challenge {}\n", k).as_bytes()).await?;
        Ok(receiver.await?)
    }

    pub async fn archive<W: std::io::Write + Send + 'static>(
        &mut self,
        prefix: Option<String>,
        state: Option<(Merkle, &[Hash])>,
        w: W,
    ) -> Result<u64, anyhow::Error> {
        debug!("archive");
        let (sender, receiver) = tokio::sync::oneshot::channel();
        *self.state.lock().await = State::Archive {
            sender: Some(sender),
            len: 0,
            conflicts: 0,
            len_n: 0,
            w: Box::new(w),
        };
        self.run_protocol().await?;
        if let Some((ref state, ref extra)) = state {
            let mut cmd = format!("archive {} {}", self.channel, state.to_base32(),);
            for e in extra.iter() {
                cmd.push_str(&format!(" {}", e.to_base32()));
            }
            if let Some(ref p) = prefix {
                cmd.push_str(" :");
                cmd.push_str(p)
            }
            cmd.push('\n');
            self.c.data(cmd.as_bytes()).await?;
        } else {
            self.c
                .data(
                    format!(
                        "archive {}{}{}\n",
                        self.channel,
                        if prefix.is_some() { " :" } else { "" },
                        prefix.unwrap_or_else(String::new)
                    )
                    .as_bytes(),
                )
                .await?;
        }
        let conflicts = receiver.await.unwrap_or(0);
        Ok(conflicts)
    }

    pub async fn run_protocol(&mut self) -> Result<(), anyhow::Error> {
        if !self.is_running {
            self.is_running = true;
            debug!("run_protocol");
            self.c
                .exec(
                    true,
                    format!(
                        "{} protocol --version {} --repository {}",
                        self.remote_cmd,
                        crate::PROTOCOL_VERSION,
                        self.path
                    ),
                )
                .await?;
            debug!("waiting for a message");
            while let Some(msg) = self.c.wait().await {
                debug!("msg = {:?}", msg);
                match msg {
                    thrussh::ChannelMsg::Success => break,
                    thrussh::ChannelMsg::WindowAdjusted { .. } => {}
                    thrussh::ChannelMsg::Eof => {}
                    thrussh::ChannelMsg::ExitStatus { exit_status } => {
                        if exit_status != 0 {
                            bail!("Remote exited with status {:?}", exit_status)
                        }
                    }
                    _ => {}
                }
            }
            debug!("run_protocol done");
        }
        Ok(())
    }

    pub async fn download_changelist<
        A,
        F: FnMut(&mut A, u64, Hash, libatomic::Merkle, bool) -> Result<(), anyhow::Error>,
    >(
        &mut self,
        mut f: F,
        a: &mut A,
        from: u64,
        paths: &[String],
    ) -> Result<HashSet<Position<Hash>>, anyhow::Error> {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(10);
        *self.state.lock().await = State::Changelist {
            sender,
            pending: Vec::new(),
        };
        self.run_protocol().await?;
        debug!("download_changelist");
        let mut command = Vec::new();
        write!(command, "changelist {} {}", self.channel, from).unwrap();
        for p in paths {
            write!(command, " {:?}", p).unwrap()
        }
        command.push(b'\n');
        self.c.data(&command[..]).await?;
        debug!("waiting ssh, command: {:?}", std::str::from_utf8(&command));
        let mut result = HashSet::new();
        while let Some(Some(m)) = receiver.recv().await {
            match m {
                super::ListLine::Change { n, h, m, tag } => f(a, n, h, m, tag)?,
                super::ListLine::Position(pos) => {
                    result.insert(pos);
                }
                super::ListLine::Error(err) => {
                    bail!(err)
                }
            }
        }
        if *self.has_errors.lock().await {
            bail!("Remote sent an error")
        }
        debug!("no msg, result = {:?}", result);
        Ok(result)
    }

    pub async fn upload_nodes(
        &mut self,
        progress_bar: ProgressBar,
        mut local: PathBuf,
        to_channel: Option<&str>,
        nodes: &[Node],
    ) -> Result<(), anyhow::Error> {
        self.run_protocol().await?;
        debug!("upload_nodes");
        for node in nodes {
            debug!("{:?}", node);
            let to_channel = if let Some(t) = to_channel {
                t
            } else {
                self.channel.as_str()
            };
            match node.node_type {
                NodeType::Change => {
                    libatomic::changestore::filesystem::push_filename(&mut local, &node.hash);
                    let mut change_file = std::fs::File::open(&local)?;
                    let change_len = change_file.metadata()?.len();
                    let mut change = thrussh::CryptoVec::new_zeroed(change_len as usize);
                    use std::io::Read;
                    change_file.read_exact(&mut change[..])?;
                    self.c
                        .data(
                            format!(
                                "apply {} {} {}\n",
                                to_channel,
                                node.hash.to_base32(),
                                change_len
                            )
                            .as_bytes(),
                        )
                        .await?;
                    self.c.data(&change[..]).await?;
                    libatomic::changestore::filesystem::pop_filename(&mut local);
                }
                NodeType::Tag => {
                    // Tag upload: send SHORT tag data to server
                    // Server will regenerate full tag file from channel state
                    libatomic::changestore::filesystem::push_tag_filename(&mut local, &node.state);

                    // Open tag file and extract short version
                    let mut tag_file = libatomic::tag::OpenTagFile::open(&local, &node.state)?;
                    let mut short_data = Vec::new();
                    tag_file.short(&mut short_data)?;

                    let channel_name = to_channel;

                    // Send tagup command: "tagup STATE CHANNEL SIZE\n"
                    self.c
                        .data(
                            format!(
                                "tagup {} {} {}\n",
                                node.state.to_base32(),
                                channel_name,
                                short_data.len()
                            )
                            .as_bytes(),
                        )
                        .await?;

                    // Send short tag data
                    self.c.data(&short_data[..]).await?;

                    libatomic::changestore::filesystem::pop_filename(&mut local);
                }
            }
            progress_bar.inc(1);
        }
        Ok(())
    }

    pub async fn download_nodes(
        &mut self,
        progress_bar: ProgressBar,
        nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
        send: &mut tokio::sync::mpsc::Sender<(Node, bool)>,
        path: &mut PathBuf,
        full: bool,
    ) -> Result<(), anyhow::Error> {
        self.download_nodes_(progress_bar, nodes, Some(send), path, full)
            .await
    }

    async fn download_nodes_(
        &mut self,
        progress_bar: ProgressBar,
        nodes: &mut tokio::sync::mpsc::UnboundedReceiver<Node>,
        sender: Option<&mut tokio::sync::mpsc::Sender<(Node, bool)>>,
        path: &mut PathBuf,
        full: bool,
    ) -> Result<(), anyhow::Error> {
        let (sender_, mut recv) = tokio::sync::mpsc::channel(100);
        let tmp_path = path.join("tmp");
        std::fs::create_dir_all(&path)?;
        let file = std::fs::File::create(&tmp_path)?;
        *self.state.lock().await = State::Changes {
            sender: Some(sender_),
            remaining_len: 0,
            path: tmp_path,
            final_path: path.clone(),
            file,
            hashes: Vec::new(),
            current: 0,
        };
        self.run_protocol().await?;
        let mut sender = sender.map(|x| x.clone());
        let t = tokio::spawn(async move {
            while let Some(node) = recv.recv().await {
                debug!("received node {:?}", node);
                progress_bar.inc(1);
                debug!("received");
                if let Some(ref mut sender) = sender {
                    sender.send((node, true)).await.unwrap_or(());
                }
            }
        });
        let mut received = false;
        while let Some(node) = nodes.recv().await {
            received = true;
            if let State::Changes { ref mut hashes, .. } = *self.state.lock().await {
                hashes.push(node);
            }
            debug!("download_node {:?} {:?}", node, full);
            match node.node_type {
                NodeType::Change if full => {
                    self.c
                        .data(format!("change {}\n", node.hash.to_base32()).as_bytes())
                        .await?;
                }
                NodeType::Change => {
                    self.c
                        .data(format!("partial {}\n", node.hash.to_base32()).as_bytes())
                        .await?;
                }
                NodeType::Tag => {
                    self.c
                        .data(format!("tag {}\n", node.state.to_base32()).as_bytes())
                        .await?;
                }
            }
        }
        if !received {
            *self.state.lock().await = State::None;
        };
        t.await?;
        debug!("done downloading {:?}", path);
        Ok(())
    }

    pub async fn update_identities(
        &mut self,
        rev: Option<u64>,
        mut path: PathBuf,
    ) -> Result<u64, anyhow::Error> {
        let (sender_, mut recv) = tokio::sync::mpsc::channel(100);
        *self.state.lock().await = State::Identities {
            sender: Some(sender_),
            buf: Vec::new(),
        };
        self.run_protocol().await?;
        if let Some(rev) = rev {
            self.c
                .data(format!("identities {}\n", rev).as_bytes())
                .await?;
        } else {
            self.c.data("identities\n".as_bytes()).await?;
        }
        let mut revision = 0;
        std::fs::create_dir_all(&path)?;
        while let Some(id) = recv.recv().await {
            path.push(&id.public_key.key);
            debug!("recv identity: {:?} {:?}", id, path);
            let mut id_file = std::fs::File::create(&path)?;
            serde_json::to_writer_pretty(&mut id_file, &id)?;
            path.pop();
            revision = revision.max(id.last_modified.timestamp());
        }
        debug!("done receiving");
        Ok(revision.try_into().unwrap())
    }
}
