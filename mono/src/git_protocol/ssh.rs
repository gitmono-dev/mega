use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use bytes::{Bytes, BytesMut};
use ceres::{
    TransportRuntime,
    infra::pack_stream::into_pack_byte_stream,
    lfs::lfs_structs::Link,
    transport::protocol::{
        ServiceType, SmartSession, TransportProtocol,
        smart::{self},
    },
};
use chrono::{DateTime, Duration, Utc};
use common::errors::ProtocolError;
use futures::{StreamExt, stream};
use russh::{
    Channel, ChannelId,
    keys::{HashAlg, PublicKey},
    server::{self, Auth, ChannelOpenHandle, Msg, Session},
};
use tokio::{io::AsyncReadExt, sync::Mutex};

use crate::git_protocol::{
    http::search_subsequence,
    protocol_error::{ssh_error_message, ssh_exit_status},
};

type ClientMap = HashMap<(usize, ChannelId), Channel<Msg>>;

#[derive(Clone)]
pub struct SshServer {
    pub clients: Arc<Mutex<ClientMap>>,
    pub id: usize,
    pub smart_protocol: Option<SmartSession>,
    pub state: TransportRuntime,
    pub data_combined: BytesMut,
}

impl server::Server for SshServer {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        let s = self.clone();
        self.id += 1;
        s
    }
}

impl server::Handler for SshServer {
    type Error = anyhow::Error;

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::info!("SshServer::channel_open_session:{}", channel.id());
        {
            let mut clients = self.clients.lock().await;
            clients.insert((self.id, channel.id()), channel);
        }
        reply.accept().await;
        Ok(())
    }

    /// # Executes a request on the SSH server.
    ///
    /// This function processes the received data from the specified channel and performs the
    /// corresponding action based on the received command.
    ///
    /// Arguments:
    /// - `self`: The current instance of the SSH server.
    /// - `channel`: The channel ID on which the request was received.
    /// - `data`: The received data from the channel.
    /// - `session`: The current SSH session.
    ///
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let data = String::from_utf8_lossy(data).trim().to_owned();
        tracing::info!("exec_request, channel:{:?}, command: {}", channel, data);
        // command exmaple:
        // Push: git-receive-pack '/path/to/repo.git'
        // Pull: git-upload-pack '/path/to/repo.git'
        // LFS HTTP Authenticate: git-lfs-authenticate '/path/to/repo.git' download/upload
        let command: Vec<_> = data.split(' ').collect();
        let path = command[1];
        let path = path.replace(".git", "").replace('\'', "");
        let service_type = ServiceType::from_str(command[0]).unwrap_or(ServiceType::UploadPack);
        let smart_protocol =
            SmartSession::new(PathBuf::from(&path), service_type, TransportProtocol::Ssh);
        match command[0] {
            "git-upload-pack" | "git-receive-pack" => {
                match smart_protocol.git_info_refs(&self.state).await {
                    Ok(res) => {
                        self.smart_protocol = Some(smart_protocol);
                        session.data(channel, res.to_vec())?;
                        session.channel_success(channel)?;
                    }
                    Err(err) => {
                        Self::send_protocol_error(session, channel, err)?;
                    }
                }
            }
            //Note that currently mega does not support pure ssh to transfer files, still relay on the https server.
            //see https://github.com/git-lfs/git-lfs/blob/main/docs/proposals/ssh_adapter.md for more details about pure ssh file transfer.
            "git-lfs-transfer" => {
                session.data(channel, "not implemented yet".as_bytes().to_vec())?;
            }
            // When connecting over SSH, the first attempt will be made to use
            // `git-lfs-transfer`, the pure SSH protocol, and if it fails, Git LFS will fall
            // back to the hybrid protocol using `git-lfs-authenticate`.
            "git-lfs-authenticate" => {
                let mut header = HashMap::new();
                let config = self.state.storage.config();
                header.insert("Accept".to_string(), "application/vnd.git-lfs".to_string());
                let link = Link {
                    href: config.lfs.ssh.http_url.clone(),
                    header,
                    expires_at: {
                        let expire_time: DateTime<Utc> =
                            Utc::now() + Duration::try_seconds(86400).unwrap();
                        expire_time.to_rfc3339()
                    },
                };
                session.data(channel, serde_json::to_vec(&link).unwrap())?;
            }
            command => tracing::error!("Not Supported command! {}", command),
        }
        Ok(())
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();

        tracing::info!("auth_publickey: {} / {}", user, fingerprint);
        let res = self
            .state
            .storage
            .user_storage()
            .search_ssh_key_finger(&fingerprint)
            .await
            .unwrap();
        if !res.is_empty() {
            tracing::info!("Client public key verified successfully!");
            Ok(Auth::Accept)
        } else {
            tracing::warn!("Client public key verification failed!");
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let smart_protocol = self.smart_protocol.as_mut().unwrap();
        tracing::info!(
            "receiving data length:{}",
            // String::from_utf8_lossy(data),
            data.len()
        );
        let service_type = smart_protocol.service_type;
        match service_type {
            ServiceType::UploadPack => {
                self.handle_upload_pack(channel, data, session).await?;
            }
            ServiceType::ReceivePack => {
                self.data_combined.extend_from_slice(data);
            }
        };
        session.channel_success(channel)?;
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(smart_protocol) = self.smart_protocol.as_mut()
            && smart_protocol.service_type == ServiceType::ReceivePack
        {
            self.handle_receive_pack(channel, session).await?;
        };

        {
            let mut clients = self.clients.lock().await;
            clients.remove(&(self.id, channel));
        }
        session.exit_status_request(channel, 0000)?;
        session.close(channel)?;
        Ok(())
    }
}

impl SshServer {
    fn send_protocol_error(
        session: &mut Session,
        channel: ChannelId,
        err: ProtocolError,
    ) -> Result<(), anyhow::Error> {
        session.data(channel, ssh_error_message(&err).into_bytes())?;
        session.exit_status_request(channel, ssh_exit_status(&err))?;
        Ok(())
    }

    async fn handle_upload_pack(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), anyhow::Error> {
        let smart_protocol = self.smart_protocol.as_mut().unwrap();

        let (mut send_pack_data, buf) = match smart_protocol
            .git_upload_pack(&self.state, &mut Bytes::copy_from_slice(data))
            .await
        {
            Ok(v) => v,
            Err(err) => {
                Self::send_protocol_error(session, channel, err)?;
                return Ok(());
            }
        };

        tracing::info!("buf is {:?}", buf);
        session
            .data(channel, String::from_utf8(buf.to_vec())?)
            .map_err(anyhow::Error::from)?;

        while let Some(chunk) = send_pack_data.next().await {
            let mut reader = chunk.as_slice();
            loop {
                let mut temp = BytesMut::new();
                temp.reserve(65500);
                let length = reader.read_buf(&mut temp).await?;
                if length == 0 {
                    break;
                }
                let bytes_out = smart_protocol.build_side_band_format(temp, length);
                session.data(channel, bytes_out.to_vec())?;
            }
        }
        session.data(channel, smart::PKT_LINE_END_MARKER.to_vec())?;
        Ok(())
    }

    async fn handle_receive_pack(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), anyhow::Error> {
        let smart_protocol = self.smart_protocol.as_mut().unwrap();
        let data = self.data_combined.split().freeze();
        let mut data_stream = Box::pin(stream::once(async move {
            Ok::<Bytes, std::convert::Infallible>(data)
        }));
        let mut report_status = Bytes::new();

        while let Some(chunk) = data_stream.next().await {
            let chunk = chunk.unwrap();

            if let Some(pos) = search_subsequence(&chunk, b"PACK") {
                let commands = smart_protocol
                    .parse_receive_pack_commands(Bytes::copy_from_slice(&chunk[..pos]));
                let remaining_bytes = Bytes::copy_from_slice(&chunk[pos..]);
                let remaining_stream =
                    stream::once(async { Ok(remaining_bytes) }).chain(data_stream);
                report_status = match smart_protocol
                    .git_receive_pack_stream(
                        &self.state,
                        commands,
                        into_pack_byte_stream(remaining_stream),
                    )
                    .await
                {
                    Ok(status) => status,
                    Err(err) => {
                        Self::send_protocol_error(session, channel, err)?;
                        return Ok(());
                    }
                };
                break;
            }
        }

        tracing::info!("report status: {:?}", report_status);
        session.data(channel, report_status.to_vec())?;
        Ok(())
    }
}
