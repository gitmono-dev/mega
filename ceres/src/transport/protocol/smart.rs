use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

use anyhow::Result;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use callisto::sea_orm_active_enums::RefTypeEnum;
use common::errors::{ProtocolError, git_to_protocol_error, mega_to_protocol_error};
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    bus::TransportRuntime,
    transport::{
        pack::PackByteStream,
        protocol::{
            Capability, ServiceType, SideBind, SmartSession, TransportProtocol, ZERO_ID,
            import_refs::RefCommand,
        },
    },
};

const LF: char = '\n';

pub const SP: char = ' ';

const NUL: char = '\0';

pub const PKT_LINE_END_MARKER: &[u8; 4] = b"0000";

// see https://git-scm.com/docs/protocol-capabilities
// The atomic, report-status, report-status-v2, delete-refs, quiet,
// and push-cert capabilities are sent and recognized by the receive-pack (push to server) process.
const RECEIVE_CAP_LIST: &str = "report-status report-status-v2 delete-refs quiet atomic no-thin ";

// The ofs-delta and side-band-64k capabilities are sent and recognized by both upload-pack and receive-pack protocols.
// The agent and session-id capabilities may optionally be sent in both protocols.
const COMMON_CAP_LIST: &str = "side-band-64k ofs-delta agent=mega/0.1.0";

// All other capabilities are only recognized by the upload-pack (fetch from server) process.
const UPLOAD_CAP_LIST: &str = "multi_ack_detailed no-done include-tag ";

impl SmartSession {
    /// # Retrieves the information about Git references (refs) for the specified service type.
    ///
    /// The function returns a `BytesMut` object containing the Git reference information.
    ///
    /// The `service_type` is extracted from the `PackProtocol` instance.
    ///
    /// The function checks if the `object_id` of the head object in the storage is zero. If it is zero,
    /// the name is set to "capabilities^{}" to include capability declarations behind a NUL on the first ref.
    /// Otherwise, the name is set to "HEAD".
    ///
    /// The `cap_list` is determined based on the `service_type` and contains the appropriate capability lists.
    ///
    /// A packet line (`pkt_line`) is constructed using the `object_id`, `name`, `NUL` delimiter, `cap_list`, and line feed (`LF`).
    /// The `pkt_line` is added to the `ref_list`.
    ///
    /// The `object_id` and `name` pairs for other refs are retrieved using the `get_ref_object_id` method from the storage.
    /// Each pair is used to construct a packet line (`pkt_line`) and added to the `ref_list`.
    ///
    /// The `build_smart_reply` method is called with the `ref_list`, `service_type`, and its string representation
    /// to build a smart reply packet line stream.
    ///
    /// Tracing information is logged regarding the response packet line stream.
    ///
    /// Finally, the constructed packet line stream is returned.
    pub async fn git_info_refs(&self, state: &TransportRuntime) -> Result<BytesMut, ProtocolError> {
        let repo_handler = self.repo_handler_with_commands(state, Vec::new()).await?;
        let service_type = self.service_type;

        // The stream MUST include capability declarations behind a NUL on the first ref.
        let (head_hash, git_refs) = repo_handler.refs_with_head_hash().await;
        let name = if head_hash == ZERO_ID {
            "capabilities^{}"
        } else {
            "HEAD"
        };
        let cap_list = match service_type {
            ServiceType::UploadPack => format!("{UPLOAD_CAP_LIST}{COMMON_CAP_LIST}"),
            ServiceType::ReceivePack => format!("{RECEIVE_CAP_LIST}{COMMON_CAP_LIST}"),
        };
        let pkt_line = format!("{head_hash}{SP}{name}{NUL}{cap_list}{LF}");
        let mut ref_list = vec![pkt_line];

        for git_ref in git_refs {
            let pkt_line = format!("{}{}{}{}", git_ref.ref_hash, SP, git_ref.ref_name, LF);
            ref_list.push(pkt_line);
        }
        let pkt_line_stream = self.build_smart_reply(&ref_list, service_type.to_string());
        tracing::info!("git_info_refs, return: --------> {:?}", pkt_line_stream);
        Ok(pkt_line_stream)
    }

    pub async fn git_upload_pack(
        &mut self,
        state: &TransportRuntime,
        upload_request: &mut Bytes,
    ) -> Result<(ReceiverStream<Vec<u8>>, BytesMut), ProtocolError> {
        let repo_handler = self.repo_handler_with_commands(state, Vec::new()).await?;

        let mut want: HashSet<String> = HashSet::new();
        let mut have: HashSet<String> = HashSet::new();
        let mut last_common_commit = String::new();

        let mut read_first_line = false;
        loop {
            let (bytes_take, pkt_line) = read_pkt_line(upload_request);
            // read 0000 to continue and read empty str to break
            if bytes_take == 0 {
                if upload_request.is_empty() {
                    break;
                } else {
                    continue;
                }
            }
            let dst = pkt_line.to_vec();
            let commands = &dst[0..4];

            match commands {
                b"want" => {
                    want.insert(String::from_utf8(dst[5..45].to_vec()).unwrap());
                }
                b"have" => {
                    have.insert(String::from_utf8(dst[5..45].to_vec()).unwrap());
                }
                b"done" => break,
                other => {
                    tracing::error!(
                        "unsupported command: {:?}",
                        String::from_utf8(other.to_vec())
                    );
                    continue;
                }
            };
            if !read_first_line {
                self.parse_capabilities(core::str::from_utf8(&dst[46..]).unwrap());
                read_first_line = true;
            }
        }

        tracing::info!(
            "want commands: {:?}\n have commands: {:?}\n caps:{:?}",
            want,
            have,
            self.capabilities
        );

        let pack_data;
        let mut protocol_buf = BytesMut::new();

        let want: Vec<String> = want.into_iter().collect();
        let have: Vec<String> = have.into_iter().collect();

        if have.is_empty() {
            pack_data = repo_handler.full_pack(want).await.map_err(|e| {
                tracing::error!(error = %e, "git upload-pack full_pack failed");
                git_to_protocol_error(e)
            })?;
            add_pkt_line_string(&mut protocol_buf, String::from("NAK\n"));
        } else if self.capabilities.contains(&Capability::MultiAckDetailed) {
            // multi_ack_detailed mode, the server will differentiate the ACKs where it is signaling that
            // it is ready to send data with ACK obj-id ready lines,
            // and signals the identified common commits with ACK obj-id common lines

            for hash in &have {
                if repo_handler.check_commit_exist(hash).await {
                    add_pkt_line_string(&mut protocol_buf, format!("ACK {hash} common\n"));
                    if last_common_commit.is_empty() {
                        last_common_commit = hash.to_string();
                    }
                }
            }
            pack_data = repo_handler
                .incremental_pack(want.clone(), have)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "git upload-pack incremental_pack failed");
                    git_to_protocol_error(e)
                })?;

            if last_common_commit.is_empty() {
                //send NAK if missing common commit
                add_pkt_line_string(&mut protocol_buf, String::from("NAK\n"));
                // need to handle rebase option, still need pack data when has no common commit
                return Ok((pack_data, protocol_buf));
            }

            for hash in want {
                if self.capabilities.contains(&Capability::NoDone) {
                    // If multi_ack_detailed and no-done are both present, then the sender is free to immediately send a pack
                    // following its first "ACK obj-id ready" message.
                    add_pkt_line_string(&mut protocol_buf, format!("ACK {hash} ready\n"));
                }
            }
            add_pkt_line_string(&mut protocol_buf, format!("ACK {last_common_commit} \n"));
        } else {
            tracing::error!("capability unsupported");
            // init a empty receiverstream
            let (_, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
            pack_data = ReceiverStream::new(rx);
            add_pkt_line_string(&mut protocol_buf, format!("ACK {last_common_commit} \n"));
        }
        Ok((pack_data, protocol_buf))
    }

    pub fn parse_receive_pack_commands(&mut self, mut protocol_bytes: Bytes) -> Vec<RefCommand> {
        let mut commands: Vec<RefCommand> = Vec::new();
        while !protocol_bytes.is_empty() {
            let (bytes_take, mut pkt_line) = read_pkt_line(&mut protocol_bytes);
            if bytes_take != 0 {
                let command = Self::parse_ref_command(&mut pkt_line);
                self.parse_capabilities(core::str::from_utf8(&pkt_line).unwrap());
                tracing::debug!(
                    "parse ref_command: {:?}, with caps:{:?}",
                    command,
                    self.capabilities
                );
                commands.push(command);
            }
        }
        commands
    }

    pub async fn git_receive_pack_stream(
        &mut self,
        state: &TransportRuntime,
        commands: Vec<RefCommand>,
        data_stream: PackByteStream,
    ) -> Result<Bytes, ProtocolError> {
        let t0 = Instant::now();
        let mut timings_ms: BTreeMap<String, u128> = BTreeMap::new();
        let mut metrics: BTreeMap<String, u128> = BTreeMap::new();
        // After receiving the pack data from the sender, the receiver sends a report
        let mut report_status = BytesMut::new();
        let mut commands = commands;
        let repo_handler = self
            .repo_handler_with_commands(state, commands.clone())
            .await?;
        let is_monorepo = repo_handler.is_monorepo();
        //1. unpack progress
        let t_unpack = Instant::now();
        let receiver = repo_handler
            .unpack_stream(&state.storage.config().pack, data_stream)
            .await?;
        timings_ms.insert(
            "unpack_stream_ms".to_string(),
            t_unpack.elapsed().as_millis(),
        );

        let t_receiver = Instant::now();
        let unpack_result = repo_handler
            .clone()
            .receiver_handler(receiver.0, receiver.1)
            .await;
        timings_ms.insert(
            "receiver_handler_ms".to_string(),
            t_receiver.elapsed().as_millis(),
        );

        // write "unpack ok\n to report"
        add_pkt_line_string(&mut report_status, "unpack ok\n".to_owned());

        let mut default_exist = repo_handler.check_default_branch().await;

        let mut unpack_failed = false;

        // 2. Tags: persist immediately. Branches: only unpack / default-branch flags here;
        //    mono and import both persist branch refs inside `finalize_receive_pack`.
        for command in commands.iter_mut() {
            if command.ref_type == RefTypeEnum::Tag {
                // just update if refs type is tag
                if let Err(e) = repo_handler.update_refs(command).await {
                    command.failed(e.to_string());
                }
            } else {
                // Updates can be unsuccessful for a number of reasons.
                // a.The reference can have changed since the reference discovery phase was originally sent, meaning someone pushed in the meantime.
                // b.The reference being pushed could be a non-fast-forward reference and the update hooks or configuration could be set to not allow that, etc.
                // c.Also, some references can be updated while others can be rejected.
                match unpack_result {
                    Ok(_) => {
                        if !default_exist {
                            command.default_branch = true;
                            default_exist = true;
                        }
                    }
                    Err(ref err) => {
                        command.failed(err.to_string());
                        unpack_failed = true;
                    }
                }
            }
        }

        // Handler was built with an early `commands.clone()`; merge loop updates (e.g.
        // `default_branch`) before finalize so import/mono metadata uses the final commands.
        repo_handler.sync_commands_after_unpack(&commands);

        let mut finalize_ms: Option<u128> = None;
        let mut bind_ms: Option<u128> = None;
        if !unpack_failed {
            let t_finalize = Instant::now();
            if let Err(e) = repo_handler.finalize_receive_pack().await {
                let msg = e.to_string();
                for c in commands.iter_mut() {
                    if c.ref_type == RefTypeEnum::Branch && c.status == "ok" {
                        c.failed(msg.clone());
                    }
                }
                return Err(mega_to_protocol_error(e));
            }
            finalize_ms = Some(t_finalize.elapsed().as_millis());

            let t_bind = Instant::now();
            self.process_commit_bindings(state, &commands).await;
            bind_ms = Some(t_bind.elapsed().as_millis());
        }

        for command in &commands {
            add_pkt_line_string(&mut report_status, command.get_status());
        }

        report_status.put(&PKT_LINE_END_MARKER[..]);
        let length = report_status.len();
        let mut buf = self.build_side_band_format(report_status, length);
        buf.put(&PKT_LINE_END_MARKER[..]);

        if let Some(ms) = finalize_ms {
            timings_ms.insert("finalize_receive_pack_ms".to_string(), ms);
        }
        if let Some(ms) = bind_ms {
            timings_ms.insert("process_commit_bindings_ms".to_string(), ms);
        }
        for (k, v) in repo_handler.receive_pack_extra_timings_ms() {
            if k.ends_with("_count") {
                metrics.insert(k, v);
            } else {
                timings_ms.insert(k, v);
            }
        }
        timings_ms.insert("total_ms".to_string(), t0.elapsed().as_millis());

        let timings_pretty = timings_ms
            .iter()
            .map(|(k, v)| format!("  - {k}: {v}ms"))
            .collect::<Vec<_>>()
            .join("\n");
        let metrics_pretty = if metrics.is_empty() {
            "  (none)".to_string()
        } else {
            metrics
                .iter()
                .map(|(k, v)| format!("  - {k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        tracing::info!(
            service = "git-receive-pack",
            repo_path = %self.repo_path.display(),
            is_monorepo = is_monorepo,
            command_count = commands.len(),
            unpack_ok = unpack_result.is_ok(),
            timings_ms = ?timings_ms,
            metrics = ?metrics,
            "\nreceive-pack timing report\n\ntimings_ms:\n{timings_pretty}\nmetrics:\n{metrics_pretty}"
        );
        Ok(buf.into())
    }

    /// # Builds the packet data in the sideband format if the SideBand/64k capability is enabled.
    ///
    /// If the `SideBand` or `SideBand64k` capability is present in the `capabilities` vector,
    /// the `from_bytes` data is transformed into the sideband format.
    /// The resulting packet data is returned in a `BytesMut` object.
    ///
    /// The `length` parameter represents the length of the `from_bytes` data.
    /// It is used to calculate the length of the transformed packet data.
    ///
    /// If the sideband format is enabled, the resulting packet data is constructed as follows:
    /// - The length of the packet data (including header) is calculated by adding 5 to the `length`.
    /// - The length value is formatted as a hexadecimal string and prepended to the `to_bytes` buffer.
    /// - The sideband type (`PackfileData`) is added as a single byte to the `to_bytes` buffer.
    /// - The `from_bytes` data is appended to the `to_bytes` buffer.
    /// - The `to_bytes` buffer containing the transformed packet data is returned.
    ///
    /// If the sideband format is not enabled, the `from_bytes` data is returned unchanged.
    pub fn build_side_band_format(&self, from_bytes: BytesMut, length: usize) -> BytesMut {
        let capabilities = &self.capabilities;
        if capabilities.contains(&Capability::SideBand)
            || capabilities.contains(&Capability::SideBand64k)
        {
            let mut to_bytes = BytesMut::new();
            let length = length + 5;
            to_bytes.put(Bytes::from(format!("{length:04x}")));
            to_bytes.put_u8(SideBind::PackfileData.value());
            to_bytes.put(from_bytes);
            return to_bytes;
        }
        from_bytes
    }

    pub fn build_smart_reply(&self, ref_list: &Vec<String>, service: String) -> BytesMut {
        let mut pkt_line_stream = BytesMut::new();
        if self.transport_protocol == TransportProtocol::Http {
            add_pkt_line_string(&mut pkt_line_stream, format!("# service={service}\n"));
            pkt_line_stream.put(&PKT_LINE_END_MARKER[..]);
        }

        for ref_line in ref_list {
            add_pkt_line_string(&mut pkt_line_stream, ref_line.to_string());
        }
        pkt_line_stream.put(&PKT_LINE_END_MARKER[..]);
        pkt_line_stream
    }

    pub fn parse_capabilities(&mut self, cap_str: &str) {
        let cap_vec: Vec<_> = cap_str.split(' ').collect();
        for cap in cap_vec {
            let res = cap.trim().parse::<Capability>();
            if let Ok(cap) = res {
                self.capabilities.insert(cap);
            }
        }
    }

    // the first line contains the capabilities
    pub fn parse_ref_command(pkt_line: &mut Bytes) -> RefCommand {
        RefCommand::new(
            read_until_white_space(pkt_line),
            read_until_white_space(pkt_line),
            read_until_white_space(pkt_line),
        )
    }

    /// Process commit bindings for successfully pushed commits
    async fn process_commit_bindings(&self, state: &TransportRuntime, commands: &[RefCommand]) {
        for command in commands {
            // Only process successful branch updates (not tags or failed commands)
            if command.ref_type == RefTypeEnum::Branch
                && command.status == "ok"
                && command.new_id != ZERO_ID
                && let Err(e) = self.bind_commit_to_user(state, &command.new_id).await
            {
                tracing::warn!("Failed to bind commit {} to user: {}", command.new_id, e);
                // Don't fail the push on binding errors
            }
        }
    }

    /// Bind a single commit to a user based on authenticated user only (username-only model)
    async fn bind_commit_to_user(
        &self,
        state: &TransportRuntime,
        commit_sha: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let commit_binding_storage = state.storage.commit_binding_storage();

        // If there is an authenticated user, bind to their username; otherwise anonymous
        let (matched_username, is_anonymous) =
            if let Some(authenticated_user) = &self.auth.authenticated_user {
                (Some(authenticated_user.username.clone()), false)
            } else {
                (None, true)
            };

        // Upsert the binding using the simplified storage API
        commit_binding_storage
            .upsert_binding(commit_sha, matched_username.clone(), is_anonymous)
            .await?;

        tracing::info!(
            "Bound commit {} -> {}",
            commit_sha,
            if is_anonymous {
                "anonymous".to_string()
            } else {
                matched_username.unwrap_or_else(|| "unknown".to_string())
            }
        );

        Ok(())
    }
}

// SmartProtocol struct removed; remaining codec helpers live on SmartSession.

fn read_until_white_space(bytes: &mut Bytes) -> String {
    let mut buf = Vec::new();
    while bytes.has_remaining() {
        let c = bytes.get_u8();
        if c.is_ascii_whitespace() || c == 0 {
            break;
        }
        buf.push(c);
    }
    String::from_utf8(buf).unwrap()
}

pub fn add_pkt_line_string(pkt_line_stream: &mut BytesMut, buf_str: String) {
    let buf_str_length = buf_str.len() + 4;
    pkt_line_stream.put(Bytes::from(format!("{buf_str_length:04x}")));
    pkt_line_stream.put(buf_str.as_bytes());
}
/// Read a single pkt-format line from the `bytes` buffer and return the line length and line bytes.
///
/// If the `bytes` buffer is empty, indicating no more data is available, the function returns a line length of 0 and an empty `Bytes` object.
///
/// The pkt-format line consists of a 4-byte length field followed by the line content. The length field specifies the total length of the line, including the length field itself. The line content is returned as a `Bytes` object.
///
/// The function first reads the 4-byte length field from the `bytes` buffer. The length value is then parsed as a hexadecimal string and converted into a `usize` value.
///
/// If the resulting line length is 0, indicating an empty line, the function returns a line length of 0 and an empty `Bytes` object.
///
/// If the line length is non-zero, the function extracts the line content from the `bytes` buffer. The extracted line content is returned as a `Bytes` object.
/// Note that this operation modifies the `bytes` buffer, consuming the bytes up to the end of the line.
///
/// # Arguments
///
/// * `bytes` - A mutable reference to a `Bytes` object representing the buffer containing pkt-format data.
///
/// # Returns
///
/// A tuple `(usize, Bytes)` representing the line length and line bytes respectively. If there is no more data available in the `bytes` buffer, the line length is 0 and an empty `Bytes` object is returned.
///
/// # Examples
///
/// ```
/// use bytes::Bytes;
/// use ceres::transport::protocol::smart::read_pkt_line;
///
/// let mut bytes = Bytes::from_static(b"000Bexample");
/// let (length, line) = read_pkt_line(&mut bytes);
/// assert_eq!(length, 11);
/// assert_eq!(line, Bytes::from_static(b"example"));
/// ```
pub fn read_pkt_line(bytes: &mut Bytes) -> (usize, Bytes) {
    if bytes.is_empty() {
        return (0, Bytes::new());
    }
    let pkt_length = bytes.copy_to_bytes(4);
    let pkt_length = usize::from_str_radix(core::str::from_utf8(&pkt_length).unwrap(), 16)
        .unwrap_or_else(|_| panic!("{pkt_length:?} is not a valid digit?"));
    if pkt_length == 0 {
        return (0, Bytes::new());
    }
    // this operation will change the original bytes
    let pkt_line = bytes.copy_to_bytes(pkt_length - 4);
    tracing::debug!("pkt line: {:?}", pkt_line);

    (pkt_length, pkt_line)
}

#[cfg(test)]
pub mod test {
    use std::{process::Command, time::Duration};

    use bytes::{Bytes, BytesMut};
    use callisto::sea_orm_active_enums::RefTypeEnum;
    use futures::future;
    use tempfile::TempDir;
    use tokio::{task, time::sleep};

    use crate::transport::protocol::{
        Capability, ServiceType, SmartSession, TransportProtocol,
        import_refs::{CommandType, RefCommand},
        smart::{add_pkt_line_string, read_pkt_line, read_until_white_space},
    };

    #[test]
    pub fn test_read_pkt_line() {
        let mut bytes = Bytes::from_static(b"001e# service=git-upload-pack\n");
        let (pkt_length, pkt_line) = read_pkt_line(&mut bytes);
        assert_eq!(pkt_length, 30);
        assert_eq!(&pkt_line[..], b"# service=git-upload-pack\n");
    }

    #[test]
    pub fn test_build_smart_reply() {
        let mock = SmartSession::new(
            std::path::PathBuf::new(),
            ServiceType::UploadPack,
            TransportProtocol::Http,
        );
        let ref_list = vec![String::from(
            "7bdc783132575d5b3e78400ace9971970ff43a18 refs/heads/master\0report-status report-status-v2 thin-pack side-band side-band-64k ofs-delta shallow deepen-since deepen-not deepen-relative multi_ack_detailed no-done object-format=sha1\n",
        )];
        let pkt_line_stream = mock.build_smart_reply(&ref_list, String::from("git-upload-pack"));
        assert_eq!(&pkt_line_stream[..], b"001e# service=git-upload-pack\n000000e87bdc783132575d5b3e78400ace9971970ff43a18 refs/heads/master\0report-status report-status-v2 thin-pack side-band side-band-64k ofs-delta shallow deepen-since deepen-not deepen-relative multi_ack_detailed no-done object-format=sha1\n0000")
    }

    #[test]
    pub fn test_add_to_pkt_line() {
        let mut buf = BytesMut::new();
        add_pkt_line_string(
            &mut buf,
            format!(
                "ACK {} common\n",
                "7bdc783132575d5b3e78400ace9971970ff43a18"
            ),
        );
        add_pkt_line_string(
            &mut buf,
            format!("ACK {} ready\n", "7bdc783132575d5b3e78400ace9971970ff43a18"),
        );
        assert_eq!(&buf.freeze()[..], b"0038ACK 7bdc783132575d5b3e78400ace9971970ff43a18 common\n0037ACK 7bdc783132575d5b3e78400ace9971970ff43a18 ready\n");
    }

    #[test]
    pub fn test_read_until_white_space() {
        let mut bytes = Bytes::from("Mega - A Monorepo Platform Engine".as_bytes());
        let result = read_until_white_space(&mut bytes);
        assert_eq!(result, "Mega");

        let mut bytes = Bytes::from("Hello,World!".as_bytes());
        let result = read_until_white_space(&mut bytes);
        assert_eq!(result, "Hello,World!");

        let mut bytes = Bytes::from("".as_bytes());
        let result = read_until_white_space(&mut bytes);
        assert_eq!(result, "");
    }

    #[test]
    pub fn test_parse_ref_update() {
        let mut bytes = Bytes::from("0000000000000000000000000000000000000000 27dd8d4cf39f3868c6eee38b601bc9e9939304f5 refs/heads/main\0".as_bytes());
        let result = SmartSession::parse_ref_command(&mut bytes);

        let command = RefCommand {
            ref_name: String::from("refs/heads/main"),
            old_id: String::from("0000000000000000000000000000000000000000"),
            new_id: String::from("27dd8d4cf39f3868c6eee38b601bc9e9939304f5"),
            status: String::from("ok"),
            error_msg: String::new(),
            command_type: CommandType::Create,
            ref_type: RefTypeEnum::Branch,
            default_branch: false,
        };
        assert_eq!(result, command);
    }

    #[test]
    pub fn test_parse_capabilities() {
        let mut mock = SmartSession::new(
            std::path::PathBuf::new(),
            ServiceType::UploadPack,
            TransportProtocol::Http,
        );
        mock.parse_capabilities("report-status-v2 side-band-64k object-format=sha10000");
        assert_eq!(
            mock.capabilities,
            std::collections::HashSet::from([Capability::ReportStatusv2, Capability::SideBand64k])
        );
    }

    async fn git_push_with_retry(repo_path: &std::path::Path) -> anyhow::Result<()> {
        let max_retries = 5;

        for attempt in 1..=max_retries {
            let status = Command::new("git")
                .args(["push", "origin", "main"])
                .current_dir(repo_path)
                .status()?;

            if status.success() {
                return Ok(());
            }

            eprintln!(
                "git push failed (attempt {}/{}) — retrying...",
                attempt, max_retries
            );

            // 1s, 2s, 4s, 8s...
            let delay = Duration::from_secs(1 << (attempt - 1));
            sleep(delay).await;
        }

        Err(anyhow::anyhow!("git push failed after retries"))
    }

    async fn init_and_push(repo_name: &str) -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let repo_path = tmp.path().join(repo_name);
        std::fs::create_dir_all(&repo_path)?;

        let remote_url = format!("http://localhost:8000/third-party/rust/src/{}", repo_name);

        // 1. git init
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&repo_path)
            .status()?;

        // 2. add a file
        std::fs::write(repo_path.join("README.md"), format!("# {}\n", repo_name))?;

        // 3. git add .
        Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .status()?;

        Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(&repo_path)
            .status()?;

        // 4. git commit
        Command::new("git")
            .args(["commit", "-m", "init commit"])
            .current_dir(&repo_path)
            .status()?;

        // 5. git remote add
        Command::new("git")
            .args(["remote", "add", "origin", &remote_url])
            .current_dir(&repo_path)
            .status()?;

        // 6. git push
        git_push_with_retry(&repo_path).await?;

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 32)]
    #[ignore]
    async fn test_dynamic_repos_push() -> anyhow::Result<()> {
        let repo_count = 64;
        let repo_names: Vec<String> = (1..=repo_count).map(|i| format!("repo{}", i)).collect();

        // push
        let tasks = repo_names.into_iter().map(|name| {
            task::spawn(async move {
                init_and_push(&name).await.unwrap();
            })
        });

        future::join_all(tasks).await;

        Ok(())
    }
}
