//! Native session and slot ownership commands.
use super::*;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Sessions<'w> {
    pub(super) session: Option<ResMut<'w, cyrus_session::SessionBoundary>>,
    pub(super) native_load: ResMut<'w, crate::session::NativeSessionLoad>,
    pub(super) install: Option<Res<'w, cyrus_assets::install::GameInstall>>,
}

impl Sessions<'_> {
    pub(super) fn handle(
        &mut self,
        command: Command,
        request: Reply,
        commands: &mut Commands,
    ) -> Outcome {
        let Self {
            session,
            native_load,
            install,
            ..
        } = self;
        let Reply { id, reply } = request;
        Outcome::Reply(match command {
            Command::SessionSlots { operation } => {
                commands.queue(move |world: &mut World| {
                    let result = crate::session::request_slots(world, operation).map(|()| json!({"pending":true})).map_err(|error| format!("{error:#}"));
                    let _ = reply.send(match result { Ok(value) => Response::ok(id, value), Err(error) => Response::err(id, error) });
                });
                return Outcome::Barrier;
            }
            Command::SessionSlotsResult { acknowledge } => {
                commands.queue(move |world: &mut World| {
                    let result = crate::session::slots_result(world, acknowledge);
                    let _ = reply.send(match result { Ok(value) => Response::ok(id, value), Err(error) => Response::err(id, format!("{error:#}")) });
                });
                return Outcome::Barrier;
            }
            Command::SessionLoad { path } => install.as_ref()
                .ok_or_else(|| anyhow::anyhow!("native session install unavailable"))
                .and_then(|install| {
                    anyhow::ensure!(session.as_ref().is_some_and(|boundary| boundary.is_idle()), "session boundary pending or result unread");
                    native_load.start(path, (**install).clone())
                })
                .map(|()| json!({"pending":true}))
                .map_err(|error| format!("{error:#}")),
            Command::SessionLoadResult { acknowledge } => {
                let completion = native_load.completion();
                if acknowledge && completion.is_none() {
                    Err("native session load has no terminal result".into())
                } else {
                    if acknowledge { let _ = native_load.acknowledge(); }
                    match completion {
                        None => Ok(json!({"pending":true})),
                        Some(Ok(())) => Ok(json!({"pending":false,"committed":true})),
                        Some(Err(error)) => Err(error),
                    }
                }
            },
            Command::SessionCapture | Command::SessionCaptureResult if native_load.busy() =>
                Err("native session load owns the pending boundary".into()),
            Command::SessionCapture => session.as_mut()
                .ok_or_else(|| anyhow::anyhow!("session boundary unavailable"))
                .and_then(|boundary| boundary.capture_live())
                .map(|()| json!({"pending":true}))
                .map_err(|error| format!("{error:#}")),
            Command::SessionCaptureResult => (|| -> anyhow::Result<serde_json::Value> {
                let boundary = session.as_mut().ok_or_else(|| anyhow::anyhow!("session boundary unavailable"))?;
                match boundary.take_result() {
                    None => Ok(json!({"pending":true})),
                    Some(Err(error)) => Err(error),
                    Some(Ok(cyrus_session::SessionBoundaryResult::Captured(snapshot))) => {
                        let document = snapshot.encode_document()?;
                        Ok(json!({"pending":false,"bytes":document.len(),"document":String::from_utf8(document)?}))
                    }
                    Some(Ok(cyrus_session::SessionBoundaryResult::Committed)) => Ok(json!({"pending":false,"committed":true})),
                }
            })().map_err(|error| format!("{error:#}")),
        _ => unreachable!("command routed to the wrong session handler"),
        })
    }
}
