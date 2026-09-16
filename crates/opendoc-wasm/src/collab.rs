//! The browser's collaboration transport: everything *inside* the socket.
//!
//! `opendoc-service` ships a Rust client, and the desktop shell uses it. This
//! crate cannot: that client is built on `tokio` and `tungstenite`, and
//! `opendoc-service` must stay out of the WebAssembly dependency graph — a
//! network daemon's transitive tree in the browser core would drag in exactly
//! what ADR 0004 and ADR 0015 keep out of it.
//!
//! So the browser splits the transport in two, and `docs/adr/0018` records the
//! decision:
//!
//! * **TypeScript owns the socket.** `apps/desktop/src/collab.ts` opens the
//!   `WebSocket`, retries it, and hands every frame here as an opaque string.
//!   It never reads one. It also owns the credential exchange, which is
//!   `fetch` against the service's HTTP API and carries no document meaning.
//! * **Rust owns every byte inside it.** This module parses each frame, drives
//!   `OpenDocApp`'s collaboration surface with it, and *builds* the frames the
//!   socket sends. A frame the page could compose is a document operation the
//!   page authored, and TypeScript authors nothing.
//!
//! The split also removes a hazard rather than merely respecting a rule. A
//! `web-sys` WebSocket owned by Rust would deliver frames in callbacks that
//! can fire while `dispatch` is inside `with_app`, and the re-entrancy guard
//! there would *drop* the frame. JavaScript cannot interrupt a synchronous
//! call into WebAssembly, so a frame handed over from a `message` listener
//! always arrives when no command is in flight.
//!
//! # The wire types are restated here, and that is checked
//!
//! [`ServerFrame`] and [`ClientFrame`] mirror `opendoc_service::protocol`
//! without importing it, the same trade `opendoc-api` already makes for
//! `OpenDocServiceRole` and `opendoc-app` for `SERVICE_DOCUMENT_FORMAT`. What
//! makes a copied definition honest is a test that compares it, so
//! `opendoc-service` writes every frame of its own protocol to
//! `crates/opendoc-service/wire/protocol-frames.json` and asserts the file
//! matches its types; [`collab_tests`](super::collab_tests) parses that same
//! file with the types below. Either side drifting fails a test.
//!
//! Note what is *not* restated: an [`Operation`] is `opendoc_merge`'s own type,
//! because `opendoc-merge` is in this graph. Only the envelope is copied.

use opendoc_app::{
    base64_decode, EditorSelection, OpenDocApp, OpenDocPresencePeer, OpenDocServiceRole,
    OpenDocServiceSession, Operation,
};
use serde::{Deserialize, Serialize};

/// Must equal `opendoc_service::PROTOCOL_VERSION`. A welcome that disagrees is
/// refused rather than guessed at: the frames below would parse and mean
/// something else.
pub const PROTOCOL_VERSION: u32 = 2;

/// How many `Submit` frames one outbox call may produce.
///
/// The outbox chunks to the service's cap, so a tail longer than the cap
/// becomes several frames rather than one oversized one the service refuses.
/// Bounded per tick anyway: a replayed hour of offline typing should reach the
/// service steadily rather than as one burst that makes the document thread
/// unreachable while it merges.
const MAX_SUBMIT_FRAMES_PER_TICK: usize = 8;

/// Outbox calls to wait for an acknowledgement before assuming it was lost.
///
/// The caller polls the outbox every 250 ms (ADR 0018), so this is ten
/// seconds. There was no timeout at all before: an `Accepted` dropped on a
/// socket that stayed open stranded everything after it for ever, because
/// `submitted_through` had already moved past it and only a socket close rolls
/// that back. Resubmitting is safe by construction — the service acknowledges
/// a byte-identical replay of a logged operation without committing anything
/// (ADR 0015, "History immutability") — so the honest recovery is to send it
/// again rather than to wait.
const ACK_TIMEOUT_TICKS: u64 = 40;

/// How many refusals this session recovers from before giving up.
///
/// A refusal is a statement about a batch, so submitting the same batch again
/// would get the same answer — but the *reason* is usually that this replica
/// and the service disagree about what the service already holds, and a fresh
/// welcome settles that from the service's own log. So a refusal now
/// resynchronises instead of wedging the session for ever (P1-8). Bounded,
/// because a refusal a welcome cannot fix would otherwise become an infinite
/// reject-reconnect loop, which is the same lie as "Reconnecting…" against a
/// service that will never answer.
const MAX_REFUSAL_RECOVERIES: u32 = 3;

// ---- the wire ------------------------------------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerFrame {
    Welcome {
        protocol_version: u32,
        document_uuid: String,
        subject: String,
        actor: String,
        role: OpenDocServiceRole,
        commit_seq: u64,
        /// base64 of canonical CBOR of the merge base.
        base_document: String,
        operations: Vec<Operation>,
        peers: Vec<WirePeer>,
        /// The largest batch this service accepts in one `Submit`. The cap is
        /// the server's and is not restated here: this client chunks its
        /// outbox to whatever number arrived, so a deployment that changes it
        /// changes what its clients send.
        max_operations_per_submit: usize,
    },
    Accepted {
        batch_id: String,
        commit_seq: u64,
        operation_ids: Vec<WireOperationId>,
    },
    Rejected {
        batch_id: String,
        code: String,
        message: String,
    },
    Committed {
        commit_seq: u64,
        subject: String,
        actor: String,
        operations: Vec<Operation>,
    },
    Presence {
        peers: Vec<WirePeer>,
    },
    Closed {
        code: String,
        message: String,
    },
    Pong,
    /// A frame this client does not know. Ignored rather than fatal: the
    /// version check on the welcome is what catches a protocol that actually
    /// changed meaning, and dropping a session over an added message type
    /// would make the service unable to grow one.
    #[serde(other)]
    Unrecognised,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientFrame {
    Submit {
        batch_id: String,
        operations: Vec<Operation>,
    },
    Presence {
        display_name: Option<String>,
        cursor_anchor: Option<String>,
        selection_anchor: Option<String>,
    },
    #[allow(dead_code)]
    Ping,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct WireOperationId {
    pub actor: String,
    pub seq: u64,
}

/// One peer as the service sees it. `subject`, `actor` and `role` are server
/// state; a peer contributes only its own display name and selection endpoints.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct WirePeer {
    pub subject: String,
    pub actor: String,
    pub display_name: String,
    pub role: OpenDocServiceRole,
    #[serde(default)]
    pub cursor_anchor: Option<String>,
    #[serde(default)]
    pub selection_anchor: Option<String>,
    pub last_seen_ms: u64,
    pub connections: u32,
}

impl WirePeer {
    fn to_presence(&self) -> OpenDocPresencePeer {
        OpenDocPresencePeer {
            subject: self.subject.clone(),
            actor: self.actor.clone(),
            display_name: self.display_name.clone(),
            role: self.role,
            cursor_anchor: self.cursor_anchor.clone(),
            selection_anchor: self.selection_anchor.clone(),
            last_seen_ms: self.last_seen_ms,
            connections: self.connections,
        }
    }
}

// ---- what the UI is told -------------------------------------------------

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// No session has been asked for.
    #[default]
    Idle,
    /// A socket is being opened and no welcome has arrived.
    Connecting,
    /// The welcome landed; this replica is a client of the service.
    Live,
    /// The socket went away and another attempt is expected.
    Reconnecting,
    /// Over, and not by accident. `notice` says why.
    Closed,
}

/// Something the user must be told, in the words the service used where there
/// are any.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Notice {
    /// A stable kind so the UI can style it without reading prose.
    pub kind: String,
    pub message: String,
    /// Whether reconnecting could plausibly fix it. `false` is the honest
    /// answer for a revoked grant, a protocol mismatch, or work this replica
    /// can no longer put on the wire.
    pub resumable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CollabStatus {
    pub phase: Phase,
    pub document_uuid: String,
    pub display_name: String,
    /// The service's commit counter as of the last frame.
    pub commit_seq: u64,
    /// The highest sequence number of this actor's operations the service has
    /// made durable.
    pub acknowledged_seq: u64,
    /// Local operations the service has not acknowledged yet.
    pub pending_operations: usize,
    /// Whether a batch would be put on the wire if there were one.
    pub can_submit: bool,
    /// Set when this session cannot go on over the socket it has and needs a
    /// fresh welcome: the service refused a batch, or a commit could not be
    /// applied. The caller owns the socket, so it is the only thing that can
    /// close one and open another; reading this clears it, exactly as
    /// `document_changed` is cleared, because it describes a moment rather
    /// than a standing fact.
    ///
    /// This is what stops a refusal being permanent. `blocked` used to be
    /// cleared only by a welcome, and nothing asked for one, so a session sat
    /// in `live` with `can_submit: false` while every keystroke piled up
    /// locally (P1-8).
    pub reconnect_requested: bool,
    /// Set when a frame changed the document. The reader is expected to
    /// re-render, and reading it clears it.
    pub document_changed: bool,
    pub notice: Option<Notice>,
    /// The service's own answers — subject, actor, role, peers, watermark —
    /// read back from the app, which is the only thing that holds them.
    pub session: Option<OpenDocServiceSession>,
    /// This user's caret, moved to where it now points after the remote work
    /// in the frames just ingested.
    ///
    /// `None` means "nothing to do": either the caller never told this driver
    /// where its caret is ([`CollabDriver::set_selection`]), or no frame since
    /// the last read moved any text, or the block the caret named is gone.
    /// Reading it clears it, exactly as `document_changed` is cleared, because
    /// it describes frames rather than a standing fact — re-applying it on a
    /// later tick would drag the caret back from wherever the user has since
    /// put it.
    ///
    /// A caller that shows a caret must apply this whenever it is `Some`. That
    /// is the whole point of it: `OpenDocApp::apply_remote_operations` has
    /// always computed the rebase, and until this field existed nothing
    /// carried the answer out, so a collaborator typing in front of your caret
    /// left it stranded at the same character index.
    pub selection: Option<EditorSelection>,
}

// ---- the driver ----------------------------------------------------------

/// The client half of the protocol: a state machine over frames.
///
/// Holds no socket and no timer. Everything it does is a pure function of the
/// frames it was given and the app it was given them for, which is what makes
/// it testable on the host with no browser at all.
#[derive(Debug, Default)]
pub struct CollabDriver {
    phase: Phase,
    document_uuid: String,
    display_name: String,
    commit_seq: u64,
    acknowledged_seq: u64,
    /// The highest local sequence number handed to *this* socket. Reset on
    /// every connection, because a new socket has seen nothing.
    submitted_through: u64,
    next_batch: u64,
    /// The service's own submit cap, as its welcome stated it. Zero until a
    /// welcome has arrived, which is also the only time this client could have
    /// anything to submit.
    submit_limit: usize,
    /// Outbox calls since this session began. The driver holds no timer, so
    /// the caller's pump is the clock, and this is what an acknowledgement
    /// timeout is measured in.
    tick: u64,
    /// The tick at which this connection began waiting for an acknowledgement
    /// it has not had. `None` means nothing is outstanding.
    awaiting_ack_since: Option<u64>,
    /// Refusals since the last accepted batch. Survives a reconnect on
    /// purpose: it is the thing that stops a refusal a welcome cannot fix from
    /// becoming an endless reconnect loop.
    refusal_recoveries: u32,
    /// See [`CollabStatus::reconnect_requested`].
    reconnect_requested: bool,
    desired_cursor: Option<String>,
    sent_cursor: Option<String>,
    desired_selection_anchor: Option<String>,
    sent_selection_anchor: Option<String>,
    announced_self: bool,
    /// Set when the service refused a batch, or when this replica can no
    /// longer produce one the service would accept. While it is set nothing is
    /// submitted: a refusal here is never transient, so retrying would only
    /// bury the reason.
    blocked: Option<Notice>,
    notice: Option<Notice>,
    document_changed: bool,
    joined: bool,
    /// Where the caller says its caret is. Handed to
    /// `OpenDocApp::apply_remote_operations`, which is the only thing that can
    /// move it correctly: rebasing a caret needs the document before the
    /// remote work and the document after it, and only the intake has both.
    selection: Option<EditorSelection>,
    /// The rebased answer, waiting to be read. See [`CollabStatus::selection`].
    rebased_selection: Option<EditorSelection>,
}

impl CollabDriver {
    /// A socket is being opened. Clears the per-connection state and leaves
    /// the per-session state (the acknowledged watermark, the joined flag)
    /// alone, because a reconnect continues the same session.
    pub fn begin(&mut self, document_uuid: &str, display_name: &str) {
        let same_document = self.joined && self.document_uuid == document_uuid;
        if !same_document {
            *self = Self::default();
        }
        self.document_uuid = document_uuid.trim().to_string();
        self.display_name = display_name.trim().to_string();
        self.phase = if same_document {
            Phase::Reconnecting
        } else {
            Phase::Connecting
        };
        // A new socket has been told nothing, so nothing has been submitted on
        // it and no presence announced on it.
        self.submitted_through = self.acknowledged_seq;
        self.sent_cursor = None;
        self.sent_selection_anchor = None;
        self.announced_self = false;
        self.awaiting_ack_since = None;
        self.reconnect_requested = false;
    }

    /// One server frame. `Err` is a frame this client could not use; the
    /// reason is also left in [`CollabStatus::notice`] so it reaches a user
    /// rather than only a caller.
    pub fn ingest(&mut self, app: &mut OpenDocApp, text: &str) -> Result<(), String> {
        let frame: ServerFrame = serde_json::from_str(text).map_err(|error| {
            let message = format!("the service sent a frame this client cannot read: {error}");
            self.notice = Some(Notice {
                kind: "unreadable-frame".to_string(),
                message: message.clone(),
                resumable: true,
            });
            message
        })?;
        match frame {
            ServerFrame::Welcome {
                protocol_version,
                document_uuid,
                subject,
                actor,
                role,
                commit_seq,
                base_document,
                operations,
                peers,
                max_operations_per_submit,
            } => self.welcome(
                app,
                protocol_version,
                document_uuid,
                subject,
                actor,
                role,
                commit_seq,
                &base_document,
                operations,
                peers,
                max_operations_per_submit,
            ),
            ServerFrame::Accepted {
                commit_seq,
                operation_ids,
                ..
            } => {
                let highest = operation_ids
                    .iter()
                    .filter(|id| id.actor == app.actor_id())
                    .map(|id| id.seq)
                    .max();
                if let Some(seq) = highest {
                    app.acknowledge_service_operations(seq);
                    self.acknowledged_seq = self.acknowledged_seq.max(seq);
                }
                if self.acknowledged_seq >= self.submitted_through {
                    self.awaiting_ack_since = None;
                }
                // A batch the service took is the only evidence that this
                // replica and the service agree about what the service holds,
                // so it is the only thing that forgives an earlier refusal.
                self.refusal_recoveries = 0;
                self.commit_seq = self.commit_seq.max(commit_seq);
                Ok(())
            }
            ServerFrame::Rejected {
                batch_id,
                code,
                message,
            } => {
                // Every refusal this service gives is a statement about the
                // batch, not about the moment: a non-dense sequence, a payload
                // under an id the log already holds, an actor that is not this
                // subject's, a role that may not write. So submitting stops at
                // once — resubmitting the same batch would only bury the
                // reason.
                //
                // But *why* the batch is wrong is almost always that this
                // replica and the service disagree about what the service
                // already holds, and a fresh welcome settles that from the
                // service's own log: it re-derives the acknowledged watermark
                // from the log the welcome carries, so the tail this replica
                // replays afterwards is the tail the service is actually
                // missing. Asking for one is therefore the recovery, and it is
                // bounded so a refusal a welcome cannot fix ends the session
                // instead of looping.
                //
                // Only a refusal that arrives while this session is still
                // submitting counts. A chunked outbox can have several batches
                // in flight, and once the first is refused the rest are the
                // same failure arriving again.
                if self.blocked.is_none() {
                    self.refusal_recoveries += 1;
                }
                let exhausted = self.refusal_recoveries >= MAX_REFUSAL_RECOVERIES;
                let tail = if exhausted {
                    format!(
                        "This session is over after {MAX_REFUSAL_RECOVERIES} refusals; the document is still here, and reconnecting is how to find out whether the service will take it."
                    )
                } else {
                    format!(
                        "Resynchronising from the service's log and trying again (attempt {} of {MAX_REFUSAL_RECOVERIES}).",
                        self.refusal_recoveries
                    )
                };
                let notice = Notice {
                    kind: format!("refused-{code}"),
                    message: format!(
                        "The service refused this replica's changes ({code}): {message}. {tail} (batch {batch_id})"
                    ),
                    resumable: !exhausted,
                };
                self.blocked = Some(notice.clone());
                self.notice = Some(notice);
                if exhausted {
                    self.phase = Phase::Closed;
                } else {
                    self.reconnect_requested = true;
                }
                Ok(())
            }
            ServerFrame::Committed {
                commit_seq,
                operations,
                ..
            } => {
                // A commit that cannot be applied is a commit this replica
                // does not have: `apply_remote_operations` is all or nothing,
                // so the whole of it is lost. Before this the session said so
                // in a notice and carried on — still `Live`, still submitting,
                // now silently disagreeing with everyone else, and with
                // `commit_seq` advanced past a commit it never folded in, so
                // nothing would ever notice (P1-9).
                //
                // Three things follow, and all three matter. Stop submitting,
                // because anything authored from here is anchored on a
                // document the service does not have. Leave `commit_seq`
                // alone, because this replica really is behind it. And ask for
                // a fresh welcome, which carries the base and the *whole* log
                // — so the lost commit comes back, which is the only way this
                // client has of re-requesting one.
                let intake = match app.apply_remote_operations(operations, self.selection.clone()) {
                    Ok(intake) => intake,
                    Err(error) => {
                        let message = format!(
                            "A commit from the service could not be applied, so this replica is behind it and has stopped sending: {error}. Resynchronising from the service's log."
                        );
                        let notice = Notice {
                            kind: "commit-not-applied".to_string(),
                            message: message.clone(),
                            resumable: true,
                        };
                        self.blocked = Some(notice.clone());
                        self.notice = Some(notice);
                        self.reconnect_requested = true;
                        return Err(message);
                    }
                };
                self.commit_seq = self.commit_seq.max(commit_seq);
                if intake.applied > 0 {
                    self.document_changed = true;
                    // The caret this replica is holding, moved by the text
                    // that just arrived in front of it. Kept on both sides:
                    // in `selection` so the *next* commit rebases from where
                    // the caret now is rather than from where it was two
                    // commits ago, and in `rebased_selection` so the caller
                    // is told to move it.
                    self.adopt_rebased(intake.selection);
                }
                Ok(())
            }
            ServerFrame::Presence { peers } => {
                // `GrantsChanged` makes the service broadcast a new presence
                // snapshot.  That snapshot is also the re-attestation of this
                // connection's role: retaining the welcome's role here would
                // leave the desktop badge and command gate stale after an
                // owner changed this subject from editor to viewer. Match both
                // server-assigned identities so another peer cannot alter the
                // local answer merely by sharing a display name or subject.
                let local_identity = app
                    .service_session()
                    .map(|session| (session.subject.clone(), session.actor.clone()));
                let reattested_role = local_identity.and_then(|(subject, actor)| {
                    peers
                        .iter()
                        .find(|peer| peer.subject == subject && peer.actor == actor)
                        .map(|peer| peer.role)
                });
                app.apply_service_presence(peers.iter().map(WirePeer::to_presence).collect());
                if let Some(role) = reattested_role {
                    app.apply_service_role(role);
                }
                Ok(())
            }
            ServerFrame::Closed { code, message } => {
                // A grant that was revoked or a credential that expired will
                // refuse the next handshake in exactly the same way, so
                // "reconnecting…" would be a lie.
                let resumable = !matches!(code.as_str(), "forbidden" | "unauthenticated");
                self.notice = Some(Notice {
                    kind: format!("closed-{code}"),
                    message: format!("The service closed this session ({code}): {message}"),
                    resumable,
                });
                if !resumable {
                    self.phase = Phase::Closed;
                }
                Ok(())
            }
            ServerFrame::Pong | ServerFrame::Unrecognised => Ok(()),
        }
    }

    /// The welcome, which is also how a reconnect resynchronises.
    ///
    /// The order matters and is the whole of the reconnect story:
    ///
    /// 1. Take what this replica holds *before* joining, because joining
    ///    replaces the document with the service's.
    /// 2. Join from the welcome — base plus the whole log — so this replica is
    ///    the service's document exactly (ADR 0007: the merged bytes are a
    ///    function of base and operation set, nothing else).
    /// 3. Read the acknowledged watermark back out of the log the welcome
    ///    carried. The service enforces dense per-actor sequences, so whatever
    ///    of this actor's work is in that log is a prefix of it, and its
    ///    highest sequence number *is* what the service has made durable.
    /// 4. Replay the tail the service never got, as operations rather than as
    ///    gestures: same ids, same causal contexts, so they are the operations
    ///    that were authored and not new ones. The normal outbox then submits
    ///    them.
    ///
    /// Step 4 is the only part that can fail, and it fails loudly. Work that
    /// cannot be replayed onto the service's log is work this replica cannot
    /// put on the wire, and saying so is the only honest answer — silently
    /// dropping it would lose a user's typing, and silently keeping it would
    /// leave this replica permanently disagreeing with everyone else.
    #[allow(clippy::too_many_arguments)]
    fn welcome(
        &mut self,
        app: &mut OpenDocApp,
        protocol_version: u32,
        document_uuid: String,
        subject: String,
        actor: String,
        role: OpenDocServiceRole,
        commit_seq: u64,
        base_document: &str,
        operations: Vec<Operation>,
        peers: Vec<WirePeer>,
        max_operations_per_submit: usize,
    ) -> Result<(), String> {
        if protocol_version != PROTOCOL_VERSION {
            let message = format!(
                "the service speaks protocol version {protocol_version} and this client speaks {PROTOCOL_VERSION}"
            );
            self.phase = Phase::Closed;
            self.notice = Some(Notice {
                kind: "protocol-version".to_string(),
                message: message.clone(),
                resumable: false,
            });
            return Err(message);
        }
        let base = decode_base_document(base_document).inspect_err(|error| {
            self.notice = Some(Notice {
                kind: "unreadable-base".to_string(),
                message: error.clone(),
                resumable: true,
            });
        })?;

        let rejoining = self.joined;
        let held: Vec<Operation> = if rejoining {
            app.local_operations_after(0)
        } else {
            Vec::new()
        };

        let session = OpenDocServiceSession::new(subject, actor, &document_uuid, role)
            .with_peers(peers.iter().map(WirePeer::to_presence).collect());
        app.join_collaboration_session(session, base, operations)
            .map_err(|error| {
                let message =
                    format!("this replica could not adopt the service's document: {error}");
                self.phase = Phase::Closed;
                self.notice = Some(Notice {
                    kind: "join-failed".to_string(),
                    message: message.clone(),
                    resumable: false,
                });
                message
            })?;

        let acknowledged = highest_local_seq(app);
        app.acknowledge_service_operations(acknowledged);
        self.acknowledged_seq = acknowledged;
        self.submitted_through = acknowledged;
        self.document_uuid = document_uuid;
        self.commit_seq = commit_seq;
        self.phase = Phase::Live;
        self.joined = true;
        self.document_changed = true;
        self.blocked = None;
        self.sent_cursor = None;
        self.sent_selection_anchor = None;
        self.announced_self = false;
        self.awaiting_ack_since = None;
        self.reconnect_requested = false;
        // The cap is the service's, and this is where it arrives. Not
        // restated: a client that guessed low would submit needlessly small
        // batches, and one that guessed high would be refused for ever.
        self.submit_limit = max_operations_per_submit.max(1);

        let unsent: Vec<Operation> = held
            .into_iter()
            .filter(|operation| operation.id.seq > acknowledged)
            .collect();
        if unsent.is_empty() {
            if rejoining {
                self.notice = Some(Notice {
                    kind: "resynchronised".to_string(),
                    message: "Reconnected and resynchronised from the service's log.".to_string(),
                    resumable: true,
                });
            }
            return Ok(());
        }
        let count = unsent.len();
        match app.apply_remote_operations(unsent, self.selection.clone()) {
            Ok(intake) => {
                self.adopt_rebased(intake.selection);
                self.notice = Some(Notice {
                    kind: "resynchronised".to_string(),
                    message: format!(
                        "Reconnected and resynchronised from the service's log; {count} change(s) made while disconnected are being resubmitted."
                    ),
                    resumable: true,
                });
            }
            Err(error) => {
                // Work this replica cannot put on the wire ends the session.
                // The notice has always said `resumable: false`, but the phase
                // stayed `Live`, so the pill read "Live" over a session that
                // could never send anything again — the one state this whole
                // module exists to avoid. The document is still here and still
                // saveable; the *session* is what is over.
                let notice = Notice {
                    kind: "unsendable-work".to_string(),
                    message: format!(
                        "Reconnected, but {count} change(s) made while disconnected could not be replayed onto the service's log, so this session is over and those changes are only in this copy: {error}"
                    ),
                    resumable: false,
                };
                self.blocked = Some(notice.clone());
                self.notice = Some(notice);
                self.phase = Phase::Closed;
            }
        }
        Ok(())
    }

    /// The socket went away. Whether anything is tried again is the caller's
    /// decision — it owns the socket and the timer — but the session's own
    /// state is rolled back to what the *service* is known to hold.
    pub fn socket_closed(&mut self, code: &str, message: &str) {
        self.submitted_through = self.acknowledged_seq;
        self.sent_cursor = None;
        self.announced_self = false;
        self.awaiting_ack_since = None;
        // The caller is about to close this socket, and a request for a fresh
        // one must not survive into the next connection.
        self.reconnect_requested = false;
        // A session already over stays over, and keeps the words that ended
        // it. This early return is what stops a socket's own close — which
        // always follows the frame that ended the session — from overwriting
        // "Disconnected, because your access was revoked" with
        // "Reconnecting…" against something nothing is going to retry. Every
        // non-resumable end sets this phase where it happens: a protocol
        // version this client does not speak, a join it could not make, work
        // it cannot replay, a refusal a welcome could not fix, and a close the
        // service sent.
        if self.phase == Phase::Closed {
            return;
        }
        self.phase = Phase::Reconnecting;
        if self.notice.as_ref().is_none_or(|notice| notice.resumable) {
            let detail = if message.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", message.trim())
            };
            self.notice = Some(Notice {
                kind: "socket-closed".to_string(),
                message: format!("The connection to the service dropped ({code}){detail}."),
                resumable: true,
            });
        }
    }

    /// Where this user's caret is, in the document's own terms.
    ///
    /// Distinct from [`Self::set_cursor`], which is the *presence* anchor: an
    /// opaque string the service relays to other people and never resolves.
    /// This one is never sent anywhere. It is handed to
    /// `OpenDocApp::apply_remote_operations` so that the caret can be moved by
    /// the remote work it is about to fold in, and the moved caret comes back
    /// in [`CollabStatus::selection`] for the caller to apply.
    ///
    /// Call it whenever the caret moves. A stale one is not harmful — the
    /// rebase maps positions in the document *before* the commit, and a caret
    /// that has since moved is simply rebased from the wrong place and
    /// replaced by the caller's next update — but a missing one means no
    /// rebase at all, which is what "the caret does not follow a
    /// collaborator's typing" looks like.
    pub fn set_selection(&mut self, selection: Option<EditorSelection>) {
        self.selection = selection;
    }

    /// Records a rebased caret, on both sides: as the caret the *next* commit
    /// rebases from, and as the answer the caller has yet to read.
    ///
    /// A `None` answer is left alone rather than written through. The intake
    /// answers `None` when the caret's block no longer exists — which is a
    /// caret this driver can no longer improve on, not an instruction to stop
    /// showing one — and when no caret was supplied at all.
    fn adopt_rebased(&mut self, rebased: Option<EditorSelection>) {
        if let Some(selection) = rebased {
            self.selection = Some(selection.clone());
            self.rebased_selection = Some(selection);
        }
    }

    /// Where this user's caret is, for the next presence frame. An opaque
    /// string to the service, which relays it and never resolves it.
    pub fn set_cursor(&mut self, anchor: Option<String>) {
        self.desired_cursor = anchor
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    /// The fixed endpoint of the active selection sent with the caret/focus.
    /// It is opaque service presence, never a document operation.
    pub fn set_selection_anchor(&mut self, anchor: Option<String>) {
        self.desired_selection_anchor = anchor
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }

    /// The frames the socket should send now, as JSON text.
    ///
    /// Called on a timer rather than from an edit hook: a local gesture
    /// becomes an operation inside `dispatch`, and the frontend module that
    /// routes gestures belongs to another surface. Polling costs one
    /// `local_operations_after` per tick and cannot miss an edit.
    pub fn outbox(&mut self, app: &mut OpenDocApp) -> Vec<String> {
        let mut frames: Vec<ClientFrame> = Vec::new();
        if self.phase != Phase::Live {
            return encode_frames(frames);
        }
        // The caller's pump is this driver's only clock.
        self.tick = self.tick.wrapping_add(1);
        self.expire_unacknowledged_submit();
        // Presence goes out even when submitting is blocked: where someone's
        // caret is remains true whether or not their edits are being accepted.
        if !self.announced_self
            || self.sent_cursor != self.desired_cursor
            || self.sent_selection_anchor != self.desired_selection_anchor
        {
            frames.push(ClientFrame::Presence {
                display_name: Some(self.display_name.clone()),
                cursor_anchor: self.desired_cursor.clone(),
                selection_anchor: self.desired_selection_anchor.clone(),
            });
            self.sent_cursor = self.desired_cursor.clone();
            self.sent_selection_anchor = self.desired_selection_anchor.clone();
            self.announced_self = true;
        }
        if self.blocked.is_some() {
            return encode_frames(frames);
        }

        // The watermark to send from is whichever is further along: what this
        // connection has put on the wire, or what the service has said is
        // durable. Taking the maximum is what makes the check below reachable
        // for both ways this can go wrong.
        let from = self.submitted_through.max(self.acknowledged_seq);
        let highest = highest_local_seq(app);
        if highest < from {
            // This replica holds fewer of its own operations than the service
            // has already made durable, so the next id it mints is one the log
            // already holds with a different payload — which the service
            // refuses, because rewriting history is exactly what it will not
            // do.
            //
            // Undo used to cause this: it rewound the operation counter.
            // ADR 0017 changed that — an undo is now the inverse of this
            // actor's own operations, submitted like any other edit — so this
            // is no longer an expected state. It is kept as a cross-check,
            // because the failure it catches is silent divergence and the
            // alternative to catching it is not noticing.
            let notice = Notice {
                kind: "history-rewritten".to_string(),
                message: format!(
                    "This replica holds its own operations only up to {highest}, but the service has already made {from} durable. The service refuses to rewrite history, so nothing further will be sent; reconnect to resynchronise from its log."
                ),
                resumable: true,
            };
            self.blocked = Some(notice.clone());
            self.notice = Some(notice);
            return encode_frames(frames);
        }
        if !app.local_operations_are_dense_after(from) {
            let notice = Notice {
                kind: "sequence-gap".to_string(),
                message: format!(
                    "This replica's operations after {from} are not a dense sequence, which the service refuses rather than stores. Nothing further will be sent on this session."
                ),
                resumable: true,
            };
            self.blocked = Some(notice.clone());
            self.notice = Some(notice);
            return encode_frames(frames);
        }
        let batch = app.local_operations_after(from);
        if batch.is_empty() {
            return encode_frames(frames);
        }
        if !self.role_allows_write(app) {
            // A pre-check, not a decision: the service decides, and would
            // refuse this on arrival. Refusing to send it keeps a viewer's
            // keystrokes from becoming a stream of refusals.
            let role = self
                .session_role(app)
                .map(|role| role.as_str().to_string())
                .unwrap_or_else(|| "none".to_string());
            self.notice = Some(Notice {
                kind: "not-an-editor".to_string(),
                message: format!(
                    "The service granted this subject the {role} role on this document, which may not write, so local changes are not being sent."
                ),
                resumable: true,
            });
            return encode_frames(frames);
        }
        // Chunked to the service's own cap. One tick can find far more than a
        // batch than the service accepts — a replayed disconnection, a large
        // paste, an import — and sending it as one frame was refused, which
        // set `blocked`, which nothing cleared, which made every keystroke
        // after it local for ever (P1-8). `chunks` never yields an empty
        // slice, and the batch is non-empty here, so every frame below carries
        // at least one operation.
        let limit = self.submit_limit.max(1);
        for chunk in batch.chunks(limit).take(MAX_SUBMIT_FRAMES_PER_TICK) {
            self.next_batch += 1;
            self.submitted_through = chunk
                .iter()
                .map(|operation| operation.id.seq)
                .max()
                .unwrap_or(self.submitted_through);
            frames.push(ClientFrame::Submit {
                batch_id: format!("{}-{}", self.document_uuid, self.next_batch),
                operations: chunk.to_vec(),
            });
        }
        if self.awaiting_ack_since.is_none() {
            self.awaiting_ack_since = Some(self.tick);
        }
        encode_frames(frames)
    }

    /// Rolls an unacknowledged submit back so the next tick sends it again.
    ///
    /// There was no acknowledgement timeout anywhere. `submitted_through`
    /// moves as a batch goes out and comes back only when the socket closes,
    /// so an `Accepted` lost on a socket that stays open stranded that batch —
    /// and everything after it — with the pill cheerfully reading "Live".
    ///
    /// Resending is safe rather than merely hopeful: an operation id already
    /// in the service's log may be resubmitted with a byte-identical payload,
    /// and that is acknowledged without committing anything (ADR 0015,
    /// "History immutability"). So the worst case of a timeout that fired too
    /// early is one redundant frame and one acknowledgement.
    fn expire_unacknowledged_submit(&mut self) {
        let Some(since) = self.awaiting_ack_since else {
            return;
        };
        if self.tick.saturating_sub(since) < ACK_TIMEOUT_TICKS {
            return;
        }
        self.awaiting_ack_since = None;
        if self.submitted_through <= self.acknowledged_seq {
            return;
        }
        let unacknowledged = self.submitted_through;
        self.submitted_through = self.acknowledged_seq;
        self.notice = Some(Notice {
            kind: "acknowledgement-timed-out".to_string(),
            message: format!(
                "The service did not acknowledge changes up to {unacknowledged}; it has confirmed {} as durable. Sending them again — the service treats an exact resend of work it already holds as a retry.",
                self.acknowledged_seq
            ),
            resumable: true,
        });
    }

    /// Leaves the session. The operation log stays — it is the document's
    /// history — but the anchor to the service goes, and with it the role the
    /// service attested.
    pub fn leave(&mut self, app: &mut OpenDocApp) {
        if self.joined {
            app.leave_collaboration_session();
        }
        *self = Self::default();
    }

    pub fn status(&mut self, app: &OpenDocApp) -> CollabStatus {
        let session = app.service_session().cloned();
        let pending_operations = if self.joined {
            app.local_operations_after(self.acknowledged_seq).len()
        } else {
            0
        };
        let can_submit =
            self.phase == Phase::Live && self.blocked.is_none() && self.role_allows_write(app);
        CollabStatus {
            phase: self.phase,
            document_uuid: self.document_uuid.clone(),
            display_name: self.display_name.clone(),
            commit_seq: self.commit_seq,
            acknowledged_seq: self.acknowledged_seq,
            pending_operations,
            can_submit,
            // Read once, like `document_changed`: it says "open a new socket
            // now", and a second reader acting on it again would drop the
            // connection the first one just made.
            reconnect_requested: std::mem::take(&mut self.reconnect_requested),
            // Read once. The caller re-renders on it, and a second reader
            // must not be told to re-render again for the same frame.
            document_changed: std::mem::take(&mut self.document_changed),
            notice: self.notice.clone(),
            session,
            // Read once, for the same reason `document_changed` is: it says
            // "move the caret now", and telling a second reader to move it
            // again on a later tick would fight the user.
            selection: std::mem::take(&mut self.rebased_selection),
        }
    }

    fn session_role(&self, app: &OpenDocApp) -> Option<OpenDocServiceRole> {
        app.service_session().map(|session| session.role)
    }

    fn role_allows_write(&self, app: &OpenDocApp) -> bool {
        self.session_role(app)
            .is_some_and(|role| role.allows_action("write"))
    }
}

fn encode_frames(frames: Vec<ClientFrame>) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| serde_json::to_string(frame).ok())
        .collect()
}

/// The highest sequence number this actor has authored locally.
///
/// Not `.last()`: the journal's order is arrival order, and after a reconnect
/// replay this actor's own operations arrive after the service's log.
fn highest_local_seq(app: &OpenDocApp) -> u64 {
    app.local_operations_after(0)
        .iter()
        .map(|operation| operation.id.seq)
        .max()
        .unwrap_or(0)
}

fn decode_base_document(encoded: &str) -> Result<opendoc_app::Document, String> {
    let bytes = base64_decode(encoded)
        .ok_or_else(|| "the welcome's merge base is not base64".to_string())?;
    opendoc_format::decode_cbor(&bytes)
        .map_err(|error| format!("the welcome's merge base is not canonical CBOR: {error}"))
}

/// The welcome's base decoder, reachable from the tests that check this
/// client reads the fixture the service crate writes.
#[cfg(test)]
pub(crate) fn decode_base_document_for_test(
    encoded: &str,
) -> Result<opendoc_app::Document, String> {
    decode_base_document(encoded)
}
